#!/usr/bin/env python3
"""Deterministic native Wayland initial-paint regression; see wayland_first_frame.md."""
import argparse
import array
import hashlib
import json
import os
from pathlib import Path
import select
import socket
import struct
import subprocess
import time


def wait_for(predicate, seconds, description):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(0.01)
    raise RuntimeError(f"timeout: {description}")


def close_fds(ancillary):
    for level, kind, data in ancillary:
        if level == socket.SOL_SOCKET and kind == socket.SCM_RIGHTS:
            fds = array.array('i')
            fds.frombytes(data[:len(data) - len(data) % fds.itemsize])
            for fd in fds:
                os.close(fd)


def send(sock, data, ancillary=()):
    try:
        sent = sock.sendmsg([data], ancillary)
        if sent < len(data):
            sock.sendall(data[sent:])
    finally:
        close_fds(ancillary)


def packets(buffer):
    while len(buffer) >= 8:
        obj, word = struct.unpack_from('=II', buffer)
        size, opcode = word >> 16, word & 65535
        if size < 8 or size % 4:
            raise RuntimeError('invalid Wayland message length')
        if len(buffer) < size:
            return
        packet = bytes(buffer[:size])
        del buffer[:size]
        yield obj, opcode, packet


def interface_bind(packet):
    # wl_registry.bind: name, string interface, version, new_id.
    length = struct.unpack_from('=I', packet, 12)[0]
    end = 16 + ((length + 3) & ~3)
    return packet[16:16 + length - 1].decode(), struct.unpack_from('=I', packet, end + 4)[0]


def probe(binary, name, root, env, upstream):
    directory = root / name
    directory.mkdir()
    marker = directory / 'app-events.txt'
    upstream_sock = proxy = client = logfile = process = None
    wire = []
    client_buffer, server_buffer = bytearray(), bytearray()
    server_ancillary = []
    interfaces = {1: 'wl_display'}
    callbacks = {}
    sync_count = 0
    configured_serials, acknowledged_serials = [], []
    gate = None
    attached_surfaces = set()
    committed_after_gate = False
    withheld = 0
    started = time.monotonic()
    try:
        upstream_sock = socket.socket(socket.AF_UNIX)
        upstream_sock.connect(upstream)
        proxy, client = socket.socketpair()
        childenv = dict(env, WAYLAND_SOCKET=str(client.fileno()))
        logfile = open(directory / 'client.log', 'w')
        process = subprocess.Popen([str(binary), str(marker)], env=childenv, pass_fds=(client.fileno(),), stdout=logfile, stderr=subprocess.STDOUT)
        client.close()
        client = None
        while time.monotonic() - started < 5:
            for source in select.select([proxy, upstream_sock], [], [], 0.02)[0]:
                data, ancillary, flags, _ = source.recvmsg(65536, socket.CMSG_SPACE(256 * 4))
                if flags & socket.MSG_CTRUNC:
                    close_fds(ancillary)
                    raise RuntimeError('truncated ancillary data')
                if not data:
                    close_fds(ancillary)
                    raise RuntimeError(f'connection closed; child exit={process.poll()}')
                if source is proxy:
                    client_buffer.extend(data)
                    for obj, opcode, packet in packets(client_buffer):
                        interface = interfaces.get(obj, '?')
                        entry = dict(direction='client', object=obj, interface=interface, opcode=opcode, size=len(packet))
                        if obj == 1 and opcode == 1:
                            interfaces[struct.unpack_from('=I', packet, 8)[0]] = 'wl_registry'
                        elif obj == 1 and opcode == 0:
                            sync_count += 1
                            callback = struct.unpack_from('=I', packet, 8)[0]
                            callbacks[callback] = sync_count
                            entry['sync'] = sync_count
                        elif interface == 'wl_registry' and opcode == 0:
                            bound, new_id = interface_bind(packet)
                            interfaces[new_id] = bound
                        elif interface == 'wl_compositor' and opcode == 0:
                            interfaces[struct.unpack_from('=I', packet, 8)[0]] = 'wl_surface'
                        elif interface == 'wl_surface' and opcode == 1:
                            buffer = struct.unpack_from('=I', packet, 8)[0]
                            entry['buffer_attach'] = buffer
                            if gate and buffer:
                                attached_surfaces.add(obj)
                                entry['nonzero_buffer_attached_after_gate'] = True
                            else:
                                attached_surfaces.discard(obj)
                        elif interface == 'wl_surface' and opcode == 6 and gate and obj in attached_surfaces:
                            committed_after_gate = True
                            entry['buffer_commit_after_gate'] = True
                        elif interface == 'xdg_wm_base' and opcode == 2:
                            interfaces[struct.unpack_from('=I', packet, 8)[0]] = 'xdg_surface'
                        elif interface == 'xdg_surface' and opcode == 4:
                            serial = struct.unpack_from('=I', packet, 8)[0]
                            acknowledged_serials.append(serial)
                            entry['ack_configure'] = serial
                        wire.append(entry)
                    send(upstream_sock, data, ancillary)
                else:
                    server_buffer.extend(data)
                    server_ancillary.extend(ancillary)
                    for obj, opcode, packet in packets(server_buffer):
                        entry = dict(direction='server', object=obj, interface=interfaces.get(obj, '?'), opcode=opcode, size=len(packet), delivered=gate is None)
                        if gate is not None:
                            withheld += 1
                            close_fds(server_ancillary)
                            server_ancillary = []
                            wire.append(entry)
                            continue
                        if interfaces.get(obj) == 'xdg_surface' and opcode == 0:
                            serial = struct.unpack_from('=I', packet, 8)[0]
                            configured_serials.append(serial)
                            entry['configure'] = serial
                        if callbacks.get(obj) == 3 and opcode == 0:
                            if not configured_serials or not set(configured_serials) & set(acknowledged_serials):
                                raise RuntimeError('final initial sync arrived without an acknowledged configure')
                            gate = dict(sync=3, callback=obj, serials=list(configured_serials), ack_serials=list(acknowledged_serials), at_s=time.monotonic()-started)
                            entry['gate_after_this_message'] = True
                        send(proxy, packet, server_ancillary)
                        server_ancillary = []
                        wire.append(entry)
            events = marker.read_text().splitlines() if marker.exists() else []
            if gate and 'initial_roundtrips_returned' in events:
                # Fixed source paints immediately. Old source must stay alive and blocked for
                # the full observation interval while no post-sync event can reach it.
                if committed_after_gate or time.monotonic() - started > gate['at_s'] + 1:
                    break
        events = marker.read_text().splitlines() if marker.exists() else []
        task = Path(f'/proc/{process.pid}/task/{process.pid}')
        gate_index = next((i for i, e in enumerate(wire) if e.get('gate_after_this_message')), None)
        after_gate = [] if gate_index is None else [e for e in wire[gate_index+1:] if e['direction'] == 'server']
        delivered_after_gate = sum(e['delivered'] for e in after_gate)
        result = dict(binary=str(binary), sha256=hashlib.sha256(binary.read_bytes()).hexdigest(), pid=process.pid,
                      gate=gate, sync_requests=sync_count, events=events, delivered_server_messages_after_gate=delivered_after_gate,
                      withheld_server_messages=withheld, buffer_committed_after_gate=committed_after_gate, alive=process.poll() is None,
                      main_wchan=(task/'wchan').read_text().strip() if task.exists() else None)
        # These are prerequisites, not an interpretation of absent marker files.
        assert gate and gate['sync'] == 3, result
        assert 'configure_dispatched' in events, result
        assert 'initial_roundtrips_returned' in events, result
        assert events.index('configure_dispatched') < events.index('initial_roundtrips_returned'), result
        if 'draw' in events:
            assert events.index('initial_roundtrips_returned') < events.index('draw'), result
            assert committed_after_gate, result
        assert process.poll() is None, result
        assert delivered_after_gate == 0, result
        result['painted'] = 'draw' in events
        return result
    finally:
        if process is not None and process.poll() is None:
            process.terminate()
            try:
                process.wait(3)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
        close_fds(server_ancillary)
        for resource in (proxy, client, upstream_sock, logfile):
            if resource is not None:
                resource.close()
        (directory/'wire.json').write_text(json.dumps(wire, indent=2)+'\n')
        (directory/'cleanup.json').write_text(json.dumps(dict(pid=process.pid if process is not None else None, reaped=process is None or process.poll() is not None, exit=process.returncode if process is not None else None))+'\n')


def bus_address(process, seconds=5):
    """Bound startup even if a daemon never prints a full address line."""
    fd = process.stdout.fileno()
    os.set_blocking(fd, False)
    address = bytearray()
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if not select.select([fd], [], [], min(0.05, max(0, deadline - time.monotonic())))[0]:
            continue
        chunk = os.read(fd, 4096 - len(address))
        if not chunk:
            raise RuntimeError('private session bus closed without an address')
        address.extend(chunk)
        if b'\n' in address:
            value = address.split(b'\n', 1)[0].decode()
            if not value.startswith('unix:'):
                raise RuntimeError('private session bus returned an invalid address')
            return value
        if len(address) >= 4096:
            raise RuntimeError('private session bus address exceeds limit')
    raise RuntimeError('timeout: private session bus address')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--before', type=Path, required=True)
    parser.add_argument('--after', type=Path, required=True)
    parser.add_argument('--evidence', type=Path, required=True)
    args = parser.parse_args()
    root = args.evidence.resolve()
    root.mkdir(mode=0o700)  # Refuse reuse, including symlink or stale evidence.
    env = {k:v for k,v in os.environ.items() if not k.startswith(('DISPLAY','WAYLAND_','XDG_','DBUS_','AT_SPI','ATSPI_','DECEBAL_','DECSETTINGS_','RDWM','SWAYSOCK','WLR_'))}
    for directory in ('home','runtime','config','cache','data'):
        (root/directory).mkdir(mode=0o700)
    env.update(HOME=str(root/'home'), XDG_RUNTIME_DIR=str(root/'runtime'), XDG_CONFIG_HOME=str(root/'config'), XDG_CACHE_HOME=str(root/'cache'), XDG_DATA_HOME=str(root/'data'), WLR_BACKENDS='headless', WLR_RENDERER='pixman', WLR_LIBINPUT_NO_DEVICES='1', DBUS_SYSTEM_BUS_ADDRESS='unix:path='+str(root/'no-system-bus'))
    # The fixture's a11y_name=None means no bridge; the private bus also isolates any compositor activation.
    bus = sway = None
    result = {}
    try:
        with open(root/'bus.log', 'w') as buslog:
            bus = subprocess.Popen(['dbus-daemon','--session','--nofork','--print-address=1'], env=env, stdout=subprocess.PIPE, stderr=buslog)
        env['DBUS_SESSION_BUS_ADDRESS'] = bus_address(bus)
        config = root/'sway.cfg'
        config.write_text('xwayland disable\noutput HEADLESS-1 resolution 1280x800 position 0,0\ndefault_border none\n')
        sway = subprocess.Popen(['sway','-d','-c',str(config)], env=env, stdout=open(root/'sway.log','w'), stderr=subprocess.STDOUT)
        ipc = wait_for(lambda: next((root/'runtime').glob(f'sway-ipc.*.{sway.pid}.sock'), None), 10, 'owned Sway IPC socket')
        env['SWAYSOCK'] = str(ipc)
        def ready():
            query = subprocess.run(['swaymsg','-r','-t','get_outputs'],env=env,capture_output=True,text=True)
            if query.returncode:
                return None
            outputs = json.loads(query.stdout)
            return outputs if any(o.get('active') and o.get('name')=='HEADLESS-1' for o in outputs) else None
        outputs = wait_for(ready, 10, 'active private HEADLESS-1')
        (root/'outputs.json').write_text(json.dumps(outputs,indent=2)+'\n')
        upstream = wait_for(lambda: next((p for p in (root/'runtime').glob('wayland-*') if not p.name.endswith('.lock')), None), 10, 'private Wayland socket')
        result['provenance'] = dict(sway_pid=sway.pid, bus_pid=bus.pid, ipc=str(ipc), wayland=str(upstream))
        result['before'] = probe(args.before.resolve(), 'before', root, env, str(upstream))
        result['after'] = probe(args.after.resolve(), 'after', root, env, str(upstream))
        assert not result['before']['painted'], result
        assert result['after']['painted'], result
        result['passed'] = True
    finally:
        for process in (sway,bus):
            if process is not None and process.poll() is None:
                process.terminate()
                try:
                    process.wait(3)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
        if bus is not None and bus.stdout is not None:
            bus.stdout.close()
        result['cleanup'] = dict(sway_reaped=sway is None or sway.poll() is not None, bus_reaped=bus is None or bus.poll() is not None)
        (root/'results.json').write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps(result,indent=2))


if __name__ == '__main__':
    main()
