# METAL COMPATIBILITY — Intel Arc 140T

## 1. How Stela decides a capability

`crates/stela-driver-gpu/src/metal/capabilities.rs` advertises a capability
only when **all three** layers provide it:

1. the Vulkan driver reports the feature/limit/format (ANV here),
2. the AIR compiler lowers the shader construct (AIR → NIR → BRW),
3. the Metal implementation realises the API path.

A refusal is recorded as a `Gap` naming the layer that lacks it, so negative
answers are explicit. A GPU family is the conjunction of its requirements.

Baseline Stela claims for **every** supported stack: Apple1 … Apple7,
their formats, programmable blending, imageblocks and tile functions (as
macOS's compositor needs Apple3 display surfaces and every Mac running
macOS 27 has an Apple7 GPU). **Metal 3 is claimed without ray tracing**
(`supportsRaytracing` refuses). Metal 4 and Apple8+ are refused as they
presuppose Apple's architecture.

For Intel that baseline is **not inherited**: ANV has never run under Stela
(`GpuStack::INTEL.vulkan == None`), so the honest table below marks almost
everything "not yet investigated". Claiming otherwise would be fabrication.

## 2. Feature matrix (Metal → Intel)

Status: **S** supported & verified · **P** partial · **N** not implemented ·
**I** infeasible · **?** not yet investigated.
"Verified" means a test in this tree actually ran. Nothing below is verified
on real hardware yet — this port has not executed a single Intel dispatch
inside Stela.

### Shader constructs (`ShaderFeatures` in `capabilities.rs`)

| Feature | NVK path | Intel status | Note |
|---|---|---|---|
| `COMPUTE` | S | **?** | BRW compute is the first target (M4) |
| `VERTEX_FRAGMENT` | S | **?** | BRW VS/FS exist; Metal ABI lowering untested on Intel |
| `TEXTURE_SAMPLE` | S | **?** | ISL surface state via ANV |
| `TEXTURE_READ_WRITE` | S | **?** | needs ANV storage images; format support decides |
| `TEXTURE_ATOMICS` | S | **?** | |
| `BUFFER_ATOMICS` | S | **?** | |
| `THREADGROUP_MEMORY` | S | **?** | Intel SLM; size limit from ANV |
| `SIMDGROUP` | S | **?** | Metal SIMD width 32 vs Intel SIMD8/16/32 choice in BRW |
| `QUADGROUP` | S | **?** | |
| `SIMDGROUP_MATRIX` | S | **?** | Intel has DPAS on Xe2+; Xe-LPG+ support differs — needs measurement |
| `ARGUMENT_BUFFERS` | S | **?** | lowered in the Metal layer; needs descriptor-set behaviour |
| `TESSELLATION` | S | **?** | Intel tessellation exists in BRW |
| `MESH` | S | **?** | Metal mesh → VK mesh shaders; ANV extension support decides |
| `FUNCTION_POINTERS` | S | **?** | |
| `DYNAMIC_LIBRARIES` | S | **?** | |
| `RAY_TRACING` | **N (refused)** | **N (refused)** | Stela refuses it on every stack; ANV is also built `-Dintel-rt=disabled` |
| `BARYCENTRICS` | S | **?** | Raster-order/barycentric path; ANV feature support decides |
| `PULL_MODEL_INTERPOLATION` | S | **?** | |
| `QUERY_LOD` | S | **?** | |
| `RASTER_ORDER_GROUPS` | S | **?** | |
| `LAYERED_RENDERING` | S | **?** | |
| `VERTEX_AMPLIFICATION` | S | **?** | |
| `DEPTH_COMPARE_SAMPLE` | S | **?** | |

### API paths (`ApiFeatures`)

| Feature | Intel status | Note |
|---|---|---|
| `HEAPS` | **?** | integrated GPU: heap = system memory region from `xe` query |
| `INDIRECT_DRAW_DISPATCH` | **?** | |
| `INDIRECT_COMMAND_BUFFERS` | **?** | needs `VK_EXT_device_generated_commands` — **ANV support is the open question** (the Metal layer names the extension) |
| `SPARSE_TEXTURES` | **?** | ANV sparse is 64 KiB blocks; `metal/intel.rs` hard-codes `SPARSE_PAGE_BYTES = 64 KiB` |
| `TIMESTAMP_COUNTERS` | **?** | |
| `STATISTIC_COUNTERS` | **?** | |
| `PROGRAMMABLE_SAMPLE_POSITIONS` | **?** | |
| `SHARED_EVENTS` | **?** | |

### Metal objects / encoders

| Area | Intel status | Note |
|---|---|---|
| Library parsing (`.metallib`) | **S (vendor-neutral)** | same code as NVIDIA; AIR is parsed by Stela |
| AIR → NIR | **S (vendor-neutral, tested)** | `src/compiler/air_to_nir.c` + hosted tests |
| NIR → machine code | **? (BRW)** | NAK for NVIDIA; BRW untried through this ABI |
| Device / queue / command buffer | **?** | |
| Render encoders | **?** | |
| Compute encoders | **?** | |
| Blit / copy | **?** | |
| Fences / events | **?** | |
| Argument buffers, binding | **?** | the largest single file (`binding.rs`, 91 KB) is vendor-neutral code |
| Heaps / resources | **?** | |
| Pixel formats | **?** | ANV format table decides; `pixel_format.rs` maps them |
| KMS / display | **?** | xe display code compiles; no modeset yet |

## 3. Where Intel is expected to differ from NVK

Known architectural differences that must be measured, not assumed:

- **Unified memory.** NVK on a discrete GPU has VRAM; Arc 140T has none.
  Heap sizing, residency and "private storage" semantics come from xe's
  memory-region query, and Metal's `.private` storage may have to map to
  system memory with cache policy, not to a staging copy.
- **Sparse resources.** ANV's 64 KiB block size is already encoded; Metal's
  sparse page sizes must be negotiated from it.
- **Device-generated commands.** NVK exposes the extension on Turing+;
  whether ANV does on Xe-LPG+ is the gating question for
  `INDIRECT_COMMAND_BUFFERS`.
- **Tile/row-major formats.** ANV (ISL) has its own tiling rules; the
  alignment constants in `metal/intel.rs` (16-byte linear pitch/offset) come
  from ISL, unlike NVK's.
- **Subgroup width.** Metal assumes 32-wide SIMD groups; BRW may pick
  SIMD8/16/32 per shader, and `SIMDGROUP` semantics must be checked against
  actual lowering before being advertised.
- **Coherent memory.** Integrated GPU reads host memory directly: most
  synchronization can be cheaper than on NVIDIA, but cache flushing
  (`PIPE_CONTROL`-class) must be correct or results will be stale.

## 4. Priority for a macOS desktop

1. Compute dispatch (M4) — proves execution.
2. Clear/blit/copy and render pass basics — the compositor's floor.
3. Texture sampling + sRGB/format conversion — window contents.
4. Vertex/fragment pipelines — window drawing.
5. Fences/timestamps — frame pacing and vsync.
6. Everything else.

A desktop that only clears to a colour is not "Metal 3 compatible"; the table
above is the honest ledger, and it stays mostly `?` until tests exist.
