# ANV GAPS — what Stela's Metal layer needs vs ANV on Arc 140T

Measured on the target machine, 2026-11-16, with the distro's Mesa 26.2.5:

```
vulkaninfo: apiVersion 1.4.354
            deviceName "Intel(R) Graphics (ARL)"   (8086:7d51, INTEGRATED_GPU)
            driverName "Intel open-source Mesa driver"
```

ANV exposes **218 device extensions** on this GPU. Stela's Metal device
enables 33 extensions when the driver offers them
(`crates/stela-driver-gpu/src/metal/native/physical.rs`, `ENABLED`) and gates
some capability bits on their presence.

## 1. The enabled set, checked against the real driver

| Extension | ANV on ARL | Metal consequence if absent |
|---|---|---|
| `VK_KHR_pipeline_executable_properties` | YES | shader introspection only |
| **`VK_KHR_unified_image_layouts`** | **NO** | performance only — see §2 |
| `VK_KHR_maintenance8` | YES | a capability at `capabilities.rs:618` reads it |
| `VK_EXT_device_generated_commands` | YES | `INDIRECT_COMMAND_BUFFERS` (Metal command buffers in kernels) |
| `VK_EXT_robustness2` | YES | — |
| `VK_EXT_dynamic_rendering_unused_attachments` | YES | — |
| `VK_EXT_extended_dynamic_state3` | YES | — |
| `VK_KHR_maintenance5` | YES | — |
| `VK_EXT_conditional_rendering` | YES | — |
| `VK_EXT_host_image_copy` | YES | texture upload path |
| `VK_EXT_mesh_shader` | YES | Metal mesh shaders |
| `VK_EXT_sample_locations` | YES | programmable sample positions |
| `VK_KHR_calibrated_timestamps` | YES | timestamp counters |
| `VK_EXT_memory_budget` | YES | heap sizing on a shared-memory GPU |
| `VK_KHR_external_memory_fd` / `external_semaphore_fd` | YES | scanout/dmabuf sharing |
| `VK_EXT_external_memory_dma_buf` | YES | display buffer import |
| `VK_EXT_shader_stencil_export` | YES | — |
| `VK_EXT_pci_bus_info` | YES | device identity |
| `VK_EXT_image_drm_format_modifier` | YES | linear/tiled scanout images |
| `VK_KHR_vertex_attribute_divisor` | YES | — |
| `VK_EXT_depth_bias_control` | YES | — |
| `VK_EXT_depth_clip_enable` | YES | — |
| `VK_KHR_line_rasterization` | YES | — |
| `VK_EXT_image_view_min_lod` | YES | — |
| `VK_EXT_vertex_input_dynamic_state` | YES | — |
| `VK_EXT_fragment_shader_interlock` | YES | raster order groups |
| **`VK_KHR_fragment_shader_barycentric`** | **NO** | Metal `BARYCENTRICS` must be refused |
| `VK_EXT_shader_atomic_float` | YES | — |
| `VK_EXT_shader_image_atomic_int64` | YES | — |
| `VK_KHR_cooperative_matrix` | YES | `SIMDGROUP_MATRIX` |
| `VK_KHR_dynamic_rendering_local_read` | YES | tile/imageblock reads |
| **`VK_NV_shader_atomic_float16_vector`** | **NO** | NVIDIA-only; nothing on Intel needs it |

**30 of 33 present.** The three absences are one performance mode, one Metal
optional feature, and one vendor-specific extension. Nothing here blocks a
desktop.

## 2. The unified-layout question, settled

`metal/resource.rs` says: "Every texture uses one image layout for its whole
life (`VK_KHR_unified_image_layouts`), so lowering never schedules layout
transitions." That reads like a hard dependency. It is not:

- Every image layout the Metal code names is `GENERAL` (18 uses) or
  `UNDEFINED` (4); barriers go `GENERAL → GENERAL`.
- `GENERAL` is legal for sampling, storage, colour and depth attachment use
  in core Vulkan, so the single-layout design is correct without the
  extension.
- What the extension buys is the driver treating that one layout as optimal
  instead of a compatibility path. In the vendored Mesa the extension exists
  in **NVK** (`src/nouveau/vulkan/nvk_physical_device.c`) and **RADV**
  (`src/amd/vulkan/radv_physical_device.c`) but **not in ANV** (no
  `src/intel/vulkan` match).

Conclusion: on Intel, expect **correct rendering with lower texture
throughput** than on NVK until either ANV implements the extension or Stela
teaches its resource layer per-use layouts. Measure before optimising.

## 3. Ranked gaps most likely to matter for a macOS desktop

1. **No ANV has ever run under Stela** — the code path exists
   (`metal/intel.rs`'s `DRIVER`, ANV in the image's driver list) but
   `GpuStack::INTEL.vulkan` is `None`. Everything above is potential until
   this is switched on.
2. **Layout performance** (§2): GENERAL everywhere on ANV.
3. **`BARYCENTRICS` must stay refused** — the capability gate already
   derives from the Vulkan feature, so it will refuse honestly; do not
   advertise it.
4. **Integrated-memory heap sizing** — Metal `.private` storage must map to
   the system-memory region xe reports (`xe::query::memory_bytes`), and
   `VK_EXT_memory_budget` is present to help.
5. **No ray tracing** — Stela refuses it for every stack, and ANV is built
   with `-Dintel-rt=disabled` here; consistent, no action.

## 4. Unknowns

- Actual ANV *feature* bits (not just extension names) on Xe-LPG+ versus
  what `capabilities.rs` reads — needs the driver to run under Stela and the
  capability gaps to be dumped.
- ANV's `VK_EXT_device_generated_commands` behaviour on Xe-LPG+ (offered,
  but Metal's indirect-command paths are untested on it).
- Format-feature coverage for Metal's format list (`pixel_format.rs`) on
  ISL; `VK_EXT_image_drm_format_modifier` presence says the scanout path
  exists, not that every format is renderable.
- Whether ANV's sparse binding block size (64 KiB, hard-coded in
  `metal/intel.rs`) matches what Metal's sparse page sizes need.

## 5. Method note

Extension presence was read from `vulkaninfo` on the target GPU, not from
Mesa's source: `vulkan-arl-140t.txt` in this repository records the summary.
Stela's requirements were read from `metal/native/physical.rs`,
`metal/capabilities.rs`, `metal/resource.rs` and `metal/intel.rs`.
