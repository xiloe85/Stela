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

### Checks actually run

| Check | Outcome |
|---|---|
| `cargo xtask gpu fetch` | **succeeded** — linux (730 MB), mesa (198 MB), llvm (141 MB), libdrm, xnu, all at their pins, Stela's series applied |
| `cargo check -p xtask` | **passed** (1m23s) with the `7d51` addition |
| `cargo test -p stela-driver-gpu --features metal` | **passed**: 639 tests, 0 failed, 1 ignored; plus 3 `kbuild` and 1 `llvm_passes` integration tests |
| `cargo test -p stela-driver-gpu --features xe-port,metal -- metal::intel` | first attempt **failed to compile** (bug 1), second **failed on the import guard** (bug 2), third **passed**: 4 Intel admission tests, 0 failed |
| `cargo test -p stela-driver-gpu --features xe-port,metal` (all) | **passed**: 654 tests, 0 failed, 1 ignored |

### Bug 1 (upstream, fixed here) — `xe-port` alone did not compile

```
error[E0432]: unresolved import `super::kbuild`
  --> crates/stela-driver-gpu/build/xe.rs:4:12
note: found an item that was configured out --> build.rs:12  (feature "nouveau-port")
```

`build/xe.rs` imports `super::kbuild`, but `mod kbuild` was gated on
`nouveau-port` only, so selecting the Intel port without NVIDIA's broke the
build script. Fixed by gating it on
`any(feature = "nouveau-port", feature = "xe-port")`. Only those two build
modules use `kbuild` (`build/nouveau.rs`, `build/xe.rs`), so the gate is
complete. This is exactly the kind of defect that hides while the Intel path
is only ever built together with NVIDIA's.

### Bug 2 (upstream, fixed here) — xe's import list omitted the driver core's device lists

After bug 1, the image compiled 352 C objects of the Intel composite and then
the import guard stopped it:

```
the imports of xe changed: newly open {"class_find_device", "device_find_child"}; now supplied []
  at crates/stela-driver-gpu/build/image.rs:86
```

`build/xe.rs`'s `OPEN` list (the imports the environment does not supply, by
service) lacked the entry `build/nouveau.rs` and `build/amdgpu.rs` both
carry, so an image that builds xe without nouveau could not satisfy the
guard. Added:

```rust
("the driver core's device lists", &["class_find_device", "device_find_child"]),
```

Those two are `STELA_UNSUPPORTED` **traps** (`__builtin_trap()`, see
`crates/stela-linux/src/compat/unsupported.h`), not graceful refusals. That
matters for Intel in a way it does not for NVIDIA: `backlight.c` and
`hwmon.c` search class devices for the panel's backlight and the GT's
sensors, and an Intel laptop with an internal panel is exactly the case that
reaches them. **Before brightness is attempted, the driver core's class and
device lists must be implemented**, or the boot will fault instead of
failing cleanly. The comment in `build/xe.rs` says so.

### Result of the Intel build

```
cargo test -p stela-driver-gpu --features xe-port,metal
  -> 654 passed; 0 failed; 1 ignored
  -> metal::intel::tests::arrow_lake_is_the_meteor_lake_platform_built_in ... ok
  -> metal::intel::tests::an_integrated_gpu_is_built_in_with_xes_platform_name ... ok
  -> metal::intel::tests::a_discrete_gpu_is_in_a_slot_or_external_with_its_link ... ok
  -> metal::intel::tests::other_devices_and_drivers_are_refused_with_the_reason ... ok
```

The Intel kernel composite (xe + the i915 display code + DRM core, 352 C
objects) builds inside Stela's Linux environment, passes the import guard,
and the Intel Metal admission logic answers correctly for this laptop's GPU.
That is Level 2 of the plan demonstrated in code, and the first time this
configuration existed at all: before these two fixes, `xe-port` could not be
built without `nouveau-port`.

### Verified against the pinned Linux tree (Linux 7.2.12)

- `0x7D51` is in `include/drm/intel/pciids.h:803` (`INTEL_ARL_H_IDS`).
- `xe_pci.c:514`: `INTEL_ARL_IDS(INTEL_VGA_DEVICE, &mtl_desc)` → ARL uses
  `mtl_desc`: `PLATFORM(METEORLAKE)`, `dma_mask_size = 46`,
  `has_display = true`, `require_force_probe = true`.
- Therefore `xe::admits(0x7d51)` succeeds and the Metal platform facts for
  this laptop's GPU are architecture `METEORLAKE`, `BuiltIn`, no transfer
  rate, not headless. Added that case to `metal/intel/tests.rs`.

### Commits on `fork` (xiloe85/Stela)

- `e3d5ebf` docs: the eight audit documents
- `687c7a1` feat(xe): ANV's host test covers Arrow Lake-P's 0x7d51

### ANV host test — blocked, with the exact reason

```
error: ANV cannot be built on this host: mesa_clc cannot be built here
(Dependency "LLVMSPIRVLib" not found (tried pkg-config and cmake);
Meson's log is target/gpu/clc/meson-log.txt)
```

So the one test that would build a BRW compute pipeline and submit a
dispatch for Arrow Lake (the `7d51` case added above) cannot run until
SPIRV-LLVM-Translator is installed on the build host. Everything else it
needs is present: LLVM/Clang 23.1.3, meson 1.12.1, ninja, glslang 16.6.0,
SPIRV-Tools 2026.4.1, libdrm 2.4.134.

### Not done in this session

- `cargo xtask gpu nvk` / `dist` (image build): not completed.
- `cargo xtask gpu anv-test`: not run (fetch incomplete; also needs
  `spirv-tools` + `spirv-llvm-translator` on the host).
- No macOS boot attempt; no USB media written; no disk touched.
