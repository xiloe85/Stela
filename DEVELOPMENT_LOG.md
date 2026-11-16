# DEVELOPMENT LOG

Chronological record of work on the Intel Arc port for the ASUS Zenbook 14
UX3405CA. Every entry states what was done, what was observed, and what
remains. Nothing is retro-actively edited into a success story.

---

## 2026-11-16 — Session 1: audit and first integration step

### Hardware audit (done, measured)

Commands: `lscpu`, `free -h`, `lsblk -o NAME,SIZE,TYPE,FSTYPE,MOUNTPOINTS`,
`lspci -nnk`, `glxinfo -B`, `lsusb`, `aplay -l`, `/sys/class/drm/*`,
`/sys/class/backlight`, `/sys/class/dmi/id/*`. `vulkaninfo` is not installed
(→ `vulkan-tools` needed). `inxi`, `qemu-system-aarch64` absent.

Findings:
- The machine is an **ASUS Zenbook 14 UX3405CA** (Arrow Lake-H), not a
  ZenBook A14 (which is the Snapdragon X model). Corrected in
  HARDWARE_COMPATIBILITY.md.
- CPU: Core Ultra 7 255H, 16 cores/16 threads (no SMT), 5.1 GHz, 42-bit
  physical addressing, **AVX2 yes / AVX-512 no**.
- GPU: `8086:7d51` Arc 140T, **Xe-LPG+ / graphics 12.74**, no VRAM, unified
  memory, currently driven by `i915`; Mesa 26.2.5 exposes GL 4.6 as
  "Mesa Intel(R) Graphics (ARL)".
- Storage: WD PC SN5000S NVMe **behind Intel VMD** (`8086:7d0b`), NVMe at
  `10000:e1:00.0`. Blocker for internal-disk boot.
- Wi-Fi `8086:7740` (Arrow Lake CNVi), Bluetooth `8087:0037` (USB),
  audio `8086:7728` with SOF.
- Display: `eDP-1` connected, `intel_backlight` present.
- RAM: 14 GiB usable, ~1 GiB free at audit time (other workloads running).

### Repository audit (done)

Cloned `tylertms/Stela` at `865c48e` into `Stela-audit/`. Read: `README.md`,
`Cargo.toml`s, `crates/stela-driver-gpu/{build.rs, build/xe.rs, src/lib.rs,
src/stack.rs, src/xe.rs, src/metal.rs, src/metal/intel.rs,
src/metal/amd.rs, src/metal/capabilities.rs}`, `crates/stela-boot/src/
engine/gpu.rs`, `crates/stela-boot-core/src/gpu_status.rs`,
`crates/stela-drivers/*` selection, `xtask/src/gpu/{nvk.rs,anv.rs,hosted.rs,
clc.rs,fetch.rs}`, `third_party/*.sparse`.

Key finding — **the port is smaller than the brief assumed**: upstream is
already building an Intel path.
- `xe-port` compiles unchanged xe + shared i915 display code and generates
  platform tables (`xe_devices.rs`).
- `src/xe.rs` implements admission, DMA mask, platform/display/dgfx facts
  and `DRM_IOCTL_XE_DEVICE_QUERY` (chip, memory regions).
- `src/metal/intel.rs` implements ANV admission plus the ANV ICD entry and
  NIR-options hook, gated on `stela_vulkan_intel`.
- `GpuStack::INTEL` exists with `vulkan: None` — ANV is *not* yet linked as a
  drawing driver, and has never run on Intel hardware under Stela.
- `xtask gpu nvk` already has ANV in the image's driver list; `xtask gpu
  anv-test` builds ANV against a no-op xe shim and dispatches via BRW.

### Fork

Forked upstream to the account's own GitHub (`xiloe85/Stela`) as requested.

### Verified against the pinned Linux tree

`third_party/linux/include/drm/xe_pciids.h` and `xe_pci.c` were read after
the sparse checkout landed, to confirm that the device this laptop has is in
xe's own table (see the follow-up entry in this log for the result).

### Source fetch

`cargo xtask gpu fetch` started (sparse submodules: linux ~730 MB checked
out; mesa, libdrm, xnu, llvm still to come; LLVM is only needed for the
image build). This is a multi-GB download and the machine is memory-tight,
so the fetch is the long pole of the session.

### Code change 1 — cover Arrow Lake in Stela's Intel host test

`xtask/src/gpu/anv.rs` `PLATFORMS` listed Tiger Lake, Alder Lake P, Meteor
Lake, DG2, Lunar Lake and Battlemage, but **not** the Arrow Lake-P part this
laptop has. Added `("7d51", "Arrow Lake P", false)`, so `cargo xtask gpu
anv-test` exercises device creation, a BRW compute pipeline and one
submission for this exact GPU.

### Not done in this session

- `cargo xtask gpu nvk` / `dist` (image build): not completed.
- `cargo xtask gpu anv-test`: not run (fetch incomplete; also needs
  `spirv-tools` + `spirv-llvm-translator` on the host).
- No macOS boot attempt; no USB media written; no disk touched.
