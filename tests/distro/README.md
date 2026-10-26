# Clean-distro check

`fedora.sh` builds macshell from source in a stock Fedora container, runs the unit tests, then runs
`macbar`, `macdock` and `machaven` under headless sway (Wayland) and under dwm built from source (X11),
and saves a screenshot of each.

```sh
docker run --rm --cap-add SYS_NICE \
  -v "$PWD":/src:ro -v "$PWD"/out:/out -v "$PWD"/tests/distro/fedora.sh:/test.sh:ro \
  fedora:latest bash /test.sh
```

Optional: mount an icon theme (`/root/.local/share/icons`), Inter (`/root/.local/share/fonts`) and a
config dir (`/cfg`, read via `MACSHELL_CONFIG`) to reproduce the original look.

## Real VM check (Debian 13)

`debian-vm.sh` runs *inside* a Debian 13 cloud-image VM (stock Rust 1.85.1, kernel 6.12, glibc 2.41):
it builds, runs the tests, then runs `macbar`/`macdock`/`machaven` under headless sway and under Debian's
own `dwm` package, saving a screenshot of each.

```sh
curl -LO https://cloud.debian.org/images/cloud/trixie/latest/debian-13-genericcloud-amd64.qcow2
qemu-img create -f qcow2 -F qcow2 -b debian-13-genericcloud-amd64.qcow2 disk.qcow2 14G
# cloud-init user-data served over HTTP (NoCloud via SMBIOS) with an ssh key for user "tester"
qemu-system-x86_64 -enable-kvm -cpu host -m 10240 -smp 8 -display none \
  -drive file=disk.qcow2,if=virtio -netdev user,id=n0,hostfwd=tcp:127.0.0.1:2222-:22 \
  -device virtio-net-pci,netdev=n0 -smbios 'type=1,serial=ds=nocloud;s=http://10.0.2.2:8000/'
tar --exclude=target --exclude=.git -czf - . | ssh -p 2222 tester@127.0.0.1 'mkdir -p macshell && tar -xzf - -C macshell'
scp -P 2222 tests/distro/debian-vm.sh tester@127.0.0.1: && ssh -p 2222 tester@127.0.0.1 bash debian-vm.sh
```

Use at least 8 GB of guest RAM: a 4 GB guest runs out of memory while linking.
