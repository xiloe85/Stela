# GPU BACKEND DESIGN — Intel Arc under Stela

## 1. What "an Intel backend" means here

Stela already has a vendor-neutral Metal implementation over an internal
Vulkan boundary (`crates/stela-driver-gpu/src/metal/`). A vendor contributes:

1. a **kernel driver** that owns the PCI function inside Stela's
   freestanding Linux environment,
2. a **Mesa Vulkan driver** linked into `stela.efi`,
3. a small **admission** function that turns the Vulkan device into Metal
   platform facts,
4. a **NIR backend** for AIR shaders.

For Intel those are `xe` (already compiles), `ANV` (already in the image's
driver list), `metal/intel.rs` (already written, `vulkan: None`), and `BRW`
(already NIR's Intel target). The port is therefore **integration and
bring-up**, not new driver architecture. That is the single most important
finding of the audit.

## 2. Candidate approaches

### A. xe + ANV + BRW (Mesa-native) — **chosen**

```
Stela Metal  →  internal Vulkan boundary  →  ANV  →  BRW (NIR backend)
                                            ↘ xe winsys (Stela's libdrm stand-in)
                                                → xe (kernel, in Stela)
                                                    → Arc 140T
AIR shaders: AIR → NIR (shared) → BRW
```

- Reuses everything Stela already ships: xe port, ANV build rules,
  AIR→NIR, the Metal layer, capability gating.
- No Vulkan validation-layer style translation of Metal to a different API:
  Metal is already lowered to Stela's own Vulkan boundary.
- Matches the architecture upstream is clearly building (xe-port feature,
  `metal/intel.rs`, `xtask gpu anv-test`, GuC firmware recipes, VBT reader).
- Risk sits in bring-up: GuC/HuC firmware, GT/GuC submission, PPGTT, power
  management, and integrated-memory behaviour under a freestanding kernel.

### B. A separate Metal→Vulkan translation layer over the host's installed
Intel Vulkan driver (ANV from the host Linux)

- Rejected. Stela is the operating system during boot; there is no host
  user space to load the distro's `libvulkan_intel.so` from, and the guest
  must talk to Stela's Metal entry points, not to a Vulkan loader. This is
  the "run it on Linux instead" approach: it would demonstrate nothing about
  Stela booting macOS and would be software-only integration theatre.

### C. Port `i915` instead of `xe`

- Rejected as the primary path: on ARL-H upstream drives the GPU with `xe`;
  `i915` is the supported driver for pre-Tiger-Lake. Porting i915 would mean
  building a second, older DRM stack for no benefit. `i915`'s *display* code
  is already compiled into Stela through xe's Makefile, which is the part
  that matters for the panel.

### D. Write a native Metal driver for Intel (Metal→Intel ISA directly)

- Rejected. It throws away NIR, BRW and ANV (hundreds of thousands of lines
  of validated Intel codegen and command emission) and would require
  implementing Intel's command streamer, samplers, image formats and
  synchronization from scratch. Nothing in Stela's structure suggests it.

## 3. Module dependency map (Intel)

```
stela-boot
  ├─ engine/gpu.rs            (Linux env, driver registration, Metal service)
  │    └─ stela-driver-gpu::stack::GpuStack::select → INTEL
  │         ├─ kernel.admits   = xe::admits          (xe PCI ID table)
  │         ├─ kernel.registers= 0  (GTTMMADR)
  │         ├─ kernel.dma_bits = xe::dma_bits        (dma_mask_size)
  │         ├─ kernel.integrated = xe::integrated    (is_dgfx)
  │         ├─ kernel.chip     = xe::query::chip     (DRM_XE_DEVICE_QUERY/config)
  │         ├─ kernel.memory_bytes = xe::query::memory_bytes (mem_regions)
  │         ├─ metal  = metal::intel::ADMISSION      (ANV ∧ xe device ∧ align)
  │         └─ vulkan = None                         ← first integration gap
  ├─ metal/capabilities.rs    (Vulkan ∧ AIR compiler ∧ implementation)
  └─ metal/native.rs          (ANV ICD entry, NIR options hook, submit)
       └─ linked archive: libvulkan_intel (BUILD: xtask gpu nvk, driver "intel")
            ├─ BRW  (src/intel/compiler)
            ├─ ISL  (surfaces)
            └─ anv_icdGetInstanceProcAddr
xe (kernel, compiled by build/xe.rs + shared i915 display)
  ├─ GuC/HuC/GSC/DMC firmware (stela-loader recipes)
  ├─ display (i915-display objects) → KMS → eDP/DP/HDMI
  └─ PCI function presented by stela-boot's PCI scan
```

## 4. Execution flow (target state)

1. **Discover**: PCI scan finds `8086:7d51`, class 0x03 →
   `Vendor::Intel` → `GpuStack::select` → `INTEL` (requires `xe-port`).
2. **Claim**: boot reads BAR0 (GTTMMADR) first page, resets the function,
   sets the DMA mask from `xe::dma_bits`, loads GuC/HuC/GSC/DMC for the
   platform from the USB firmware set (`stela repair` fetches them host-side).
3. **Kernel**: `stela_linux_start` registers xe; Linux matches the function;
   xe probes: GT/GuC init, PPGTT, VRAM-as-system-memory region, display init.
4. **Vulkan**: ANV opens the xe render node through Stela's DRM ioctl path,
   enumerates the physical device (integrated, ARL/MTL platform facts).
5. **Metal**: `metal::intel::admit_device` accepts it; capabilities are the
   conjunction of ANV features, AIR/BRW constructs and the Metal layer.
6. **Guest**: macOS's Metal calls arrive over the call gate; command
   buffers lower to the Vulkan boundary; ANV submits to the GuC; fences
   come back through Stela's IRQ/event path.
7. **Display**: KMS scanout on eDP; if the panel is not drivable from xe,
   fall back to the "copy frames another GPU drew" path.

## 5. Memory design (16 GB, no VRAM)

- Integrated GPU: `Kernel.memory_bytes` answers xe's system-memory region,
  never a fabricated VRAM size; the Metal heap is sized from it minus a
  reserve.
- No staging copies: buffers come from Stela's pinned physical page source
  (`stela-driver-base::bus::dma::memory`), so ANV's BOs are already mapped
  where the GPU reads them. Copies are only for must-be-linear cases.
- Shader cache: the NIR/pipeline caches (`metal/compiler/nir_cache.rs`,
  `library_cache.rs`) must be bounded; the image also carries LLVM and the
  translator. Cap on-disk/in-RAM caches and evict least-recently-used.
- Refuse early: allocation failure returns a diagnostic naming the request
  and the remaining region rather than panicking, because a 16 GB machine
  will hit it.

## 6. Shader translation design

`AIR → NIR` is shared and unchanged. For Intel:
- `metal/compiler.rs` hands NIR to BRW (`brw_compile_*` via ANV's pipeline
  creation) instead of NAK/ACO.
- Intel-specific NIR options are supplied by Stela's own hook
  (`compat/mesa/anv_nir_options.c` → `stela_anv_nir_options`), already
  referenced by `metal/intel.rs`'s `DRIVER`.
- Subgroup/threadgroup semantics: Metal SIMD-group width 32 vs Intel's
  SIMD8/16/32 selection is a BRW decision; capability bits
  (`ShaderFeatures::SIMDGROUP`, `QUADGROUP`, `THREADGROUP_MEMORY`) are only
  advertised once the constructs actually lower.

## 7. Testing strategy

| Level | Test | Hardware |
|---|---|---|
| S | `metal::intel` admission unit tests (TGL, DG2, ARL) | none |
| S | `xe::admits/platform/dma_bits` unit tests over the generated table | none |
| H | `cargo xtask gpu anv-test` — ANV + BRW dispatch on a no-op xe shim, per platform, every xe ioctl checked against Stela's xe | none (shim) |
| R | same ANV build linked against real libdrm, run on `/dev/dri/renderD128` | **real Arc 140T** |
| R | Stela boot with `xe-port`: PCI claim, GuC load, DRM probe in the boot log | **real Arc 140T** |
| G | macOS boot, then Metal feature probes | real |

## 8. Why A, in one sentence

Because Stela's Metal layer is already vendor-neutral, upstream has already
built xe, ANV, BRW wiring and the admissions, and the only defensible work
left is to finish the integration and bring up firmware/display/submission
on real hardware — every alternative either duplicates validated Intel
codegen or abandons the goal of booting macOS.
