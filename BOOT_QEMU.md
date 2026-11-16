# BOOTING stela.efi IN QEMU

Reproducible lane for booting the image on this machine without touching any
disk, ESP or Linux install. Captures Stela's own serial log, which is where
every line of its boot output goes (COM1, `0x3F8`).

## Prerequisites

```sh
sudo pacman -S --needed qemu-system-x86 edk2-ovmf
```

The image must be a **diagnostics** build for `--host-check` to run:

```sh
export PATH="$HOME/.cargo/bin:$PATH"      # rustup 1.99.1 + musl + uefi targets
STELA_XE=1 cargo xtask boot build --release --diagnostics
```

`STELA_XE=1` adds Intel's driver (see `INTEL_PORT_ROADMAP.md`); drop it for an
NVIDIA-only image.

## Layout

```sh
mkdir -p /tmp/stela-esp/EFI/BOOT /tmp/stela-esp/EFI/stela
cp target/boot/x86_64-unknown-uefi/release/stela-deploy.efi \
   /tmp/stela-esp/EFI/BOOT/BOOTX64.EFI
printf -- '--host-check --deadline 60\n' > /tmp/stela-esp/EFI/stela/stela.cfg
truncate -s 256M /tmp/stela-disk.img
printf 'label: gpt\n,64M,L\n' | sfdisk /tmp/stela-disk.img
cp /usr/share/edk2/x64/OVMF_VARS.4m.fd /tmp/stela-vars.fd
```

Notes that cost time to learn:

- `stela.cfg` lives at `\EFI\stela\stela.cfg`. Without it the boot stops with
  `STELA FATAL stela.cfg starts with neither --system nor --host-check`.
- `--host-check` refuses a non-diagnostics image
  (`STELA FATAL host checks require a diagnostics build`).
- QEMU's default CPU (`QEMU Virtual CPU version 2.5+`) has no SSSE3/SSE4/POPCNT,
  so the `cpu` check fails and the run later dies at
  `STELA FATAL native IRQ state contract unavailable: ErrorPointers`. Use
  `-cpu max`.

## Run

```sh
qemu-system-x86_64 -M q35 -cpu max -m 4096 -smp 2 \
  -device qemu-xhci -device usb-kbd \
  -drive if=pflash,format=raw,readonly=on,file=/usr/share/edk2/x64/OVMF_CODE.4m.fd \
  -drive if=pflash,format=raw,file=/tmp/stela-vars.fd \
  -drive format=raw,file=fat:rw:/tmp/stela-esp,media=disk \
  -drive file=/tmp/stela-disk.img,format=raw,media=disk \
  -serial file:/tmp/stela-serial.log -display none -no-reboot
```

Read `/tmp/stela-serial.log` afterwards; it is plain text.

## What it showed (2026-11-16, Intel image, 205,270,016-byte `stela.efi`)

```
STELA UEFI MOUNT version=0.4.2
HOST version=0.4.2 cpu="QEMU ..." ram=4.3GB firmware="EDK II"
FIRMWARE connected 163 of 166 handles, left 3 network controllers to Stela's drivers
DISPLAY firmware mode 1280x800 Bgr ... 27 modes ... EDID prefers 1280x800 at 74.994 Hz
BOOT IMAGE snapshot physical=0x784a5000 bytes=0x36b7000 release=ok
HOST PCI 0000:00:01.0 1234:1111 class=030000 rev=02 subsystem=1af4:1100: GPU
UNSUPPORTED host GPU 0000:00:01.0 1234:1111: no Stela GPU backend for this PCI vendor
CHECK cpu PASS x86-64-v2 floor_missing=[] la57=true
CHECK tsc PASS hz=3686399012 invariant=false source=PmTimer
CHECK native-memory PASS WB native pages: zero/read/write, CPU pin lifetime/reclaim, ...
CHECK apic PASS mode=xAPIC base=0xfee00900 timer_hz=1000004048 arat=true
CHECK display PASS 1280x800 stride=1280 BGR base=0x80000000 bytes=0x3e8000 bars=7 read_back_mismatches=0
HOST CHECK END pass=6 fail=3 skip=2
```

So the image boots, takes over the firmware, enumerates the host, scans PCI,
classifies the display controller, draws colour bars into the framebuffer and
reads them back. The three remaining failures are environmental:

| Check | Result | Why |
|---|---|---|
| `storage` / `gpt` | FAIL, `disks=0` | the VM's disks did not come up as internal NVMe/AHCI disks for the storage drivers |
| `interrupts` | FAIL, one-shot fired after 11,035 µs instead of 10,000 | QEMU's timer granularity against a 10 ms expectation (periodic was correct: 100 in 100 ms) |
| `keyboard` | FAIL, `no keyboard was brought up` | the USB keyboard was not bound before the check ran |

## What this lane can and cannot prove

- **Can**: firmware handoff, host-model enumeration, the PCI census and the
  vendor-stack selection path (`GpuStack::select`), display modes, framebuffer
  readback, APIC/interrupt delivery, native-memory paging.
- **Cannot**: anything about the real Arc 140T. QEMU's display adapter is
  `1234:1111`, not `8086:7d51`; no GuC firmware, no xe probe, no ANV device.
  Passing this lane says nothing about Intel GPU initialisation.
