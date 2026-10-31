#!/usr/bin/env python3
"""Ownership checks for the private Wayland fixture; no compositor or desktop connection."""
import importlib.util
import os
from pathlib import Path
import socket
import tempfile
import struct
import threading
import unittest

SPEC = importlib.util.spec_from_file_location("popup_fixture", Path(__file__).parent / "e2e/wayland_compact_popup.py")
FIXTURE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(FIXTURE)


class CurrentProcess:
    pid = os.getpid()

    def poll(self):
        return None


class OwnershipTests(unittest.TestCase):
    def test_only_an_open_listening_socket_is_owned(self):
        with tempfile.TemporaryDirectory(prefix="popup-ownership-") as directory:
            path = Path(directory) / "wayland-test"
            with socket.socket(socket.AF_UNIX) as listener:
                listener.bind(str(path))
                self.assertFalse(FIXTURE.socket_owned(CurrentProcess(), path))
                listener.listen()
                self.assertTrue(FIXTURE.socket_owned(CurrentProcess(), path))
            self.assertFalse(FIXTURE.socket_owned(CurrentProcess(), path))

    def test_regular_and_missing_files_are_not_sockets(self):
        with tempfile.TemporaryDirectory(prefix="popup-ownership-") as directory:
            path = Path(directory) / "wayland-test"
            self.assertFalse(FIXTURE.socket_owned(CurrentProcess(), path))
            path.write_text("not a compositor")
            self.assertFalse(FIXTURE.socket_owned(CurrentProcess(), path))

    def test_exited_owner_is_rejected(self):
        class Exited(CurrentProcess):
            def poll(self):
                return 0

        with tempfile.TemporaryDirectory(prefix="popup-ownership-") as directory:
            path = Path(directory) / "wayland-test"
            with socket.socket(socket.AF_UNIX) as listener:
                listener.bind(str(path))
                listener.listen()
                self.assertFalse(FIXTURE.socket_owned(Exited(), path))


OUTPUT_TRACE = """
[1.0][rs] <- wl_output@13.name, (Some("HEADLESS-2"))
[1.1][rs] <- wl_output@14.name, (Some("HEADLESS-1"))
[2.0][rs] -> zwlr_layer_shell_v1@6.get_layer_surface(zwlr_layer_surface_v1@19, wl_surface@18, wl_output@13, 3, Some("decdash"))
[2.1][rs] -> zwlr_layer_shell_v1@6.get_layer_surface(zwlr_layer_surface_v1@25, wl_surface@24, wl_output@14, 3, Some("decebal-popup-outside"))
"""
FRAME_TRACE = """
[1.0][rs] -> wl_subcompositor@7.get_subsurface(wl_subsurface@21, wl_surface@20, wl_surface@18)
[2.0][rs] -> wl_shm_pool@23.create_buffer(wl_buffer@31, 0, 520, 392, 2080, 0)
[3.0][rs] -> wl_surface@20.frame(wl_callback@32)
[3.1][rs] -> wl_surface@20.attach(wl_buffer@31, 0, 0)
[3.2][rs] -> wl_surface@20.commit()
[4.0][rs] <- wl_callback@32.done, (1234)
[4.1][rs] <- wl_buffer@31.release, ()
[5.0][rs] -> wl_surface@20.frame(wl_callback@33)
[5.1][rs] -> wl_surface@20.attach(wl_buffer@31, 0, 0)
[5.2][rs] -> wl_surface@20.commit()
[6.0][rs] <- wl_callback@33.done, (1235)
"""


class ProtocolProofTests(unittest.TestCase):
    def test_requested_output_and_catcher_are_resolved_to_names(self):
        failures = []
        evidence = FIXTURE.check_output_identity(OUTPUT_TRACE, failures)
        self.assertEqual(failures, [])
        self.assertEqual(evidence["host_outputs"], ["HEADLESS-2"])
        self.assertEqual(evidence["catcher_outputs"], ["HEADLESS-1"])
        # A non-null output alone cannot pass: it must be the requested output.
        failures = []
        swapped = OUTPUT_TRACE.replace('Some("HEADLESS-2")', 'Some("wrong-output")')
        FIXTURE.check_output_identity(swapped, failures)
        self.assertTrue(any("requested output identity" in item for item in failures))

    def test_card_commit_needs_its_own_frame_and_buffer_release(self):
        failures = []
        evidence = FIXTURE.check_card_frames(FRAME_TRACE, failures, "test")
        self.assertEqual(failures, [])
        self.assertEqual(evidence["card_commits"], 2)
        self.assertEqual(evidence["completed_callbacks"], 2)
        for broken, expected in (
            (FRAME_TRACE.replace("wl_surface@20.frame(wl_callback@33)", "wl_surface@18.frame(wl_callback@33)"), "no frame request"),
            (FRAME_TRACE.replace("[4.1][rs] <- wl_buffer@31.release, ()", ""), "reused before compositor release"),
            (FRAME_TRACE.replace("[4.0][rs] <- wl_callback@32.done, (1234)", ""), "before callback"),
        ):
            failures = []
            FIXTURE.check_card_frames(broken, failures, "test")
            self.assertTrue(any(expected in item for item in failures), failures)

    def test_four_live_same_size_card_buffers_fail_the_bound(self):
        added = "".join(f"[2.{i}][rs] -> wl_shm_pool@23.create_buffer(wl_buffer@{40+i}, 0, 520, 392, 2080, 0)\n" for i in range(3))
        trace = FRAME_TRACE.replace("[3.0]", added + "[3.0]", 1)
        failures = []
        FIXTURE.check_card_frames(trace, failures, "test")
        self.assertTrue(any("4 live same-size" in item for item in failures), failures)


class VirtualPointerWireTests(unittest.TestCase):
    def test_string_alignment_and_request_framing(self):
        encoded = FIXTURE.wl_string("wl_seat")
        self.assertEqual(encoded, b"\x08\0\0\0wl_seat\0")
        self.assertEqual(FIXTURE.wl_read_string(encoded), ("wl_seat", 12))
        self.assertEqual(FIXTURE.wl_message(7, 8), struct.pack("=II", 7, 8 << 16 | 8))
        for broken in (b"", b"\x04\0\0\0abc", b"\x04\0\0\0abcd"):
            with self.assertRaises(ValueError):
                FIXTURE.wl_read_string(broken)
        with self.assertRaises(ValueError):
            FIXTURE.wl_message(1, 0, b"x")

    def mock_server(self, path, missing_manager=False):
        # A tiny *local Unix protocol peer*, not a compositor or input service. It verifies request ordering
        # and sends registry/seat/sync events; no desktop sockets, processes or physical devices are used.
        listener = socket.socket(socket.AF_UNIX)
        listener.bind(str(path))
        listener.listen()
        listener.settimeout(2)
        records, errors = [], []

        def exact(connection, count):
            result = bytearray()
            while len(result) < count:
                part = connection.recv(count - len(result))
                if not part:
                    raise EOFError
                result.extend(part)
            return bytes(result)

        def serve():
            try:
                # socket_owned makes a credential-check connection before the real protocol connection.
                for _ in range(2):
                    connection, _ = listener.accept()
                    with connection:
                        connection.settimeout(2)
                        registry = manager = seat = None
                        while True:
                            try:
                                ident, header = struct.unpack("=II", exact(connection, 8))
                                size, opcode = header >> 16, header & 65535
                                if size < 8 or size % 4:
                                    raise ValueError("bad request header")
                                payload = exact(connection, size - 8)
                            except EOFError:
                                break
                            records.append((ident, opcode, payload))
                            if ident == 1 and opcode == 1:
                                registry = struct.unpack("=I", payload)[0]
                                globals_ = [(10, "wl_seat", 1)]
                                if not missing_manager:
                                    globals_.append((11, "zwlr_virtual_pointer_manager_v1", 2))
                                for name, interface, version in globals_:
                                    event = FIXTURE.wl_uints(name) + FIXTURE.wl_string(interface) + FIXTURE.wl_uints(version)
                                    connection.sendall(FIXTURE.wl_message(registry, 0, event))
                            elif ident == 1 and opcode == 0:
                                callback = struct.unpack("=I", payload)[0]
                                connection.sendall(FIXTURE.wl_message(callback, 0, FIXTURE.wl_uints(0)))
                            elif ident == registry and opcode == 0:
                                interface, end = FIXTURE.wl_read_string(payload, 4)
                                version, bound = struct.unpack_from("=II", payload, end)
                                self.assertEqual(version, 1)
                                if interface == "wl_seat":
                                    seat = bound
                                    connection.sendall(FIXTURE.wl_message(seat, 0, FIXTURE.wl_uints(0)))
                                elif interface == "zwlr_virtual_pointer_manager_v1":
                                    manager = bound
                            elif ident == manager and opcode == 0:
                                suggested_seat, pointer = struct.unpack("=II", payload)
                                self.assertEqual(suggested_seat, seat)
                                self.assertIsNotNone(pointer)
                                connection.sendall(FIXTURE.wl_message(seat, 0, FIXTURE.wl_uints(1)))
            except BaseException as error:
                errors.append(error)
            finally:
                listener.close()

        thread = threading.Thread(target=serve, daemon=True)
        thread.start()
        return thread, records, errors

    def test_private_virtual_pointer_roundtrip_waits_for_pointer_capability(self):
        with tempfile.TemporaryDirectory(prefix="popup-pointer-wire-") as directory:
            path = Path(directory) / "wayland-test"
            thread, records, errors = self.mock_server(path)
            pointer = None
            try:
                pointer = FIXTURE.VirtualPointer(CurrentProcess(), path)
                self.assertEqual(pointer.capabilities, 1)
            finally:
                if pointer:
                    pointer.close()
                thread.join(timeout=3)
            self.assertFalse(thread.is_alive())
            self.assertEqual(errors, [])
            self.assertEqual([(ident, opcode) for ident, opcode, _ in records],
                             [(1, 1), (1, 0), (2, 0), (2, 0), (1, 0), (4, 0), (1, 0), (7, 8), (4, 1)])

    def test_missing_virtual_pointer_protocol_fails_instead_of_skipping(self):
        with tempfile.TemporaryDirectory(prefix="popup-pointer-wire-") as directory:
            path = Path(directory) / "wayland-test"
            thread, _, errors = self.mock_server(path, missing_manager=True)
            try:
                with self.assertRaisesRegex(RuntimeError, "lacks zwlr_virtual_pointer_manager_v1"):
                    FIXTURE.VirtualPointer(CurrentProcess(), path)
            finally:
                thread.join(timeout=3)
            self.assertFalse(thread.is_alive())
            self.assertEqual(errors, [])


if __name__ == "__main__":
    unittest.main()
