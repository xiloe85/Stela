# INTEL PORT ROADMAP

Target: ASUS Zenbook 14 UX3405CA, Core Ultra 7 255H, **Arc 140T (`8086:7d51`,
Xe-LPG+ / gfx 12.74)**, 16 GB shared memory, macOS via Stela.

Status vocabulary: **done** / **in progress** / **not started** / **blocked**.

## M1 — Complete technical audit  → **done (this pass)**

| Task | Status | Evidence |
|---|---|---|
| Clone upstream | done | `Stela-audit/`, commit `865c48e` |
| Build the project | **blocked** | needs sparse submodules (mesa/linux/llvm ≈ GBs); fetch started, see DEVELOPMENT_LOG |
| Understand source architecture | done | `ARCHITECTURE.md` |
| Identify NVIDIA-specific components | done | `ARCHITECTURE.md` §5 |
| Identify Intel PCI ID | done | `8086:7d51`, Arc 140T, Xe-LPG+ 12.74 |
| Verify Linux Vulkan support | **partial** | GL 4.6 via Mesa 26.2.5 works; `vulkaninfo` not installed → install `vulkan-tools` |
| Identify architectural blockers | done | `KNOWN_LIMITATIONS.md` |

## M2 — Intel backend design  → **done (this pass)**

`GPU_BACKEND_DESIGN.md`: four approaches compared; **xe + ANV + BRW** chosen,
with the module map, execution flow, memory design and test strategy.

## M3 — Intel GPU initialization  → **not started (upstream ~60 %)**

Already upstream: xe compiles with shared i915 display; tables generated;
admission + device query implemented; ANV admitted into the Metal layer.
Missing, in order:

1. Enable `xe-port` in the image (`crates/stela-boot/Cargo.toml` currently
   requests only `nouveau-port`), and prove `cargo check` + boot.
2. Confirm the pinned `xe_pci.c` carries `0x7d51` and what platform name it
   reports (expect MTL). Add a subplatform/ARL name if it is worth it.
3. Wire GuC/HuC/GSC/DMC firmware selection for ARL into the claim path
   (recipes exist; the caller exists for nouveau only).
4. Bring up xe probe inside Stela's Linux environment: BAR0 mapping, DMA
   mask 39 bits, interrupts/IRQ domain, PPGTT, memory region.
5. Display: hand the VBT (OpRegion reader exists, no caller) to xe's display
   code; get a modeset on eDP.

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
   (blocked on host packages: `spirv-tools`, `spirv-llvm-translator`, see
   TESTING.md).
2. Add `7d51` to `xtask/src/gpu/anv.rs`'s platform list so the Intel test
   covers this laptop's exact GPU.
3. Add an Arrow Lake admission test next to the Tiger Lake/DG2 ones.
4. Enable `xe-port` in `stela-boot` behind a feature and build the image.
