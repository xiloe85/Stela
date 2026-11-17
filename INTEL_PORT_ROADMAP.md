# INTEL PORT ROADMAP

Target: ASUS Zenbook 14 UX3405CA, Core Ultra 7 255H, **Arc 140T (`8086:7d51`,
Xe-LPG+ / gfx 12.74)**, 16 GB shared memory, macOS via Stela.

Status vocabulary: **done** / **in progress** / **not started** / **blocked**.

## M1 — Complete technical audit  → **done (this pass)**

| Task | Status | Evidence |
|---|---|---|
| Clone upstream | done | `Stela-audit/`, commit `865c48e` |
| Build the project | **done** | `stela.efi` 158 MB and `stela-deploy.efi` 50.8 MB (PE32+ EFI x86-64) linked from LLVM + the 1257-object Mesa archive (nouveau, intel, amd) + the guest kext. Needed rustup's `x86_64-unknown-linux-musl` (Mesa's Rust cross build) and `x86_64-unknown-uefi` (the image), which Arch's rustc does not ship |
| Understand source architecture | done | `ARCHITECTURE.md` |
| Identify NVIDIA-specific components | done | `ARCHITECTURE.md` §5 |
| Identify Intel PCI ID | done | `8086:7d51`, Arc 140T, Xe-LPG+ 12.74 |
| Verify Linux Vulkan support | **partial** | GL 4.6 via Mesa 26.2.5 works; `vulkaninfo` not installed → install `vulkan-tools` |
| Identify architectural blockers | done | `KNOWN_LIMITATIONS.md` |

## M2 — Intel backend design  → **done (this pass)**

`GPU_BACKEND_DESIGN.md`: four approaches compared; **xe + ANV + BRW** chosen,
with the module map, execution flow, memory design and test strategy.

## M3 — Intel GPU initialization  → **in progress (selection and image build proven, probe not run)**

Done: the Intel-only configuration compiles (`build.rs`'s `kbuild` gate,
`build/xe.rs`'s import list), an Arrow Lake admission test passes, the xe
firmware family is wired into the loader bundle, and **the real machine now
answers the selection question**: `HOST GPU 0000:00:02.0 8086:7d51: xe will bind
it` (`boot-log-hardware-2026-11-16.log`, `DEVELOPMENT_LOG.md` session 4). That is
the admission test on the physical Arc 140T; it is not a bound device.
Not done: xe probe under Stela, firmware load, GuC/HuC/GSC claim, any modeset.

Already upstream: xe compiles with shared i915 display; tables generated;
admission + device query implemented; ANV admitted into the Metal layer.
Status of the missing steps:

1. **compiles, not default** — `crates/stela-boot`'s `xe` feature already
   pulls `stela-drivers/xe-port`, and
   `cd crates/stela-boot && cargo check --features xe --target
   x86_64-unknown-uefi` finishes green (3m57s, `BOOT_XE_EXIT=0`,
   DEVELOPMENT_LOG session 7). The shipped image still requests only
   `nouveau-port`, so an Intel GPU is refused before the claim; flipping that
   default belongs with the probe, not before it, so no machine loses the
   boot it has today.
2. **done** — the pinned table holds `0x7D51` (`INTEL_ARL_H_IDS`,
   `include/drm/intel/pciids.h`) and `xe_pci.c:514` binds it to `&mtl_desc`.
   xe therefore reports **METEORLAKE** for this GPU: 46-bit DMA mask,
   `is_dgfx` false, force-probe. Asserted by tests in `stack.rs` and `xe.rs`;
   `xtask/src/gpu/anv.rs` already listed `7d51`, so ANV's host lane covers it.
3. **done (already upstream)** — `gpu/before.rs`'s `XE` part calls
   `stela_loader::host_firmware::recipes::require_xe`, and that table's
   `"Meteor Lake or Arrow Lake"` entry holds `0x7d51` and asks for
   `i915/mtl_guc_70.bin`, `i915/mtl_huc_gsc.bin`, `i915/mtl_gsc_1.bin` and
   `i915/mtl_dmc.bin`, all pinned by digest from linux-firmware
   `a2dcb924…`. `recipes.rs` already tests `xe_files(0x7d51)`. What is still
   missing is the four blobs **on the stick's bundle** — the boot will now say
   so by name instead of failing generically.
4. Bring up xe probe inside Stela's Linux environment: BAR0 mapping, DMA
   mask 46 bits (not 39 — the descriptor's `dma_mask_size`), interrupts/IRQ
   domain, PPGTT, memory region.
5. Display: hand the VBT (OpRegion reader exists, no caller) to xe's display
   code; get a modeset on eDP.
6. **done** — the step names its phase before entering it (`GPU START
   phase=claim|linux|probe`, flushed to the USB log), so a start that never
   returns names where it stopped instead of hanging silently.
7. **artifact built and on the stick** — `STELA_XE=1 cargo xtask boot build
   --release` produces `stela.efi` (204,379,648 bytes, sha256 `08174ac6…`) and
   `stela-deploy.efi` (52,928,000 bytes). Both carry the four Meteor Lake
   firmware names and the phase lines. The stick's `EFI/BOOT/BOOTX64.EFI` is
   now the xe deploy image (`eba04fb7…`); the NVIDIA-only loader it replaced
   is kept at `~/Downloads/stela-backup/BOOTX64-nvidia-2026-11-17.efi`
   (`c521aafc…`). Not yet booted.

Acceptance: Stela's boot log shows xe binding `8086:7d51`, GT/GuC
initialised, a DRM render node present, and KMS presenting a frame.

## M4 — Shader execution  → **not started**

Order of work:
1. `cargo xtask gpu anv-test` for MTL-class platforms (add `7d51`) on the
   no-op shim: device creation, BRW compute pipeline, one dispatch.
2. Link ANV against real libdrm and dispatch on the real render node; compare
   results against a CPU reference.
3. AIR → NIR → BRW: compile a trivial Metal shader, dispatch, read back,
   compare with CPU. **This is the milestone that proves hardware execution.**

Acceptance: a kernel's output matches a CPU reference bit-for-bit, with the
dispatch visible as GPU work (not a CPU path).

## M5 — Graphics pipeline  → **not started**

Textures, render targets, graphics pipelines, resource binding, fences,
command encoders, shader libraries. Automated tests per feature; every
capability bit granted only after its test passes.

## M6 — Stela integration  → **partial (design done)**

Backend selection by vendor is already correct (`GpuStack::select`); NVIDIA
stays untouched; unsupported GPUs refuse with a reason
(`Adapter::unavailable_reason`). Remaining: `gpu_status.rs`'s user-facing
text is NVIDIA-only, and `GpuStack::draws()` must stay false for Intel until
ANV genuinely submits — no silent software fallback.

## M7 — macOS boot  → **not started**

Prerequisites that are *not* GPU work: a boot disk visible to Stela. On this
laptop the SSD is behind Intel VMD, so the first boots should use an external
USB/NVMe disk. Bring-up order: firmware entry → host hardware model → memory
map → storage → installer → first boot → logs.

## M8 — Desktop and performance  → **not started**

Accelerated compositing, texture acceleration, smooth window rendering,
memory stability on 16 GB. Measure, do not assume.

## M9 — Additional hardware  → **not started**

Wi-Fi (CNVi `8086:7740`), Bluetooth (`8087:0037`), audio (SOF `8086:7728`),
brightness, sleep/wake, battery, external displays. All secondary; Stela
supports none of these on any machine today.

## Sequencing rule

Do not start M7 before M4 passes. A macOS boot without a working GPU proves
nothing about this project and burns days in log archaeology.

## Immediate next actions

1. Finish `cargo xtask gpu fetch`, then `cargo xtask gpu anv-test --jobs 2`
   (host packages `spirv-tools`, `spirv-as`, `glslangValidator` and `bwrap`
   are present now; `spirv-llvm-translator` still has to answer).
2. ~~Add `7d51` to `xtask/src/gpu/anv.rs`~~ — already listed ("Arrow Lake P";
   xe's own descriptor calls the family Meteor Lake).
3. ~~Add an Arrow Lake admission test~~ — done in `stack.rs`/`xe.rs`.
4. ~~Enable `xe-port` in `stela-boot` behind a feature and build~~ — the
   feature exists and the image checks green with it; making it the default
   waits on item 4 of M3.
