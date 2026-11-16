# HARDWARE COMPATIBILITY — target laptop

All values below were **measured on the machine**, 2026-11-16, with
`lscpu`, `free`, `lsblk`, `lspci -nnk`, `glxinfo -B`, `lsusb`, `aplay -l`,
`/sys/class/drm/*`, `/sys/class/backlight`, `/sys/class/dmi/id/*`.

## 0. Identity correction (important)

The request said "ASUS ZenBook A14". The firmware says otherwise:

```
sys_vendor    : ASUSTeK COMPUTER INC.
product_name  : ASUS Zenbook 14 UX3405CA_UX3405CA
product_family: ASUS Zenbook 14
board_name    : UX3405CA
```

*ZenBook A14* is the **Snapdragon X (arm64)** model. This machine is a
**Zenbook 14 UX3405CA**: Intel Core Ultra 7 255H (Arrow Lake-H), x86-64.
That is the only configuration Stela could ever target, so the correction is
good news; but the model name matters for firmware, drivers and support.

## 1. CPU — supported

| Fact | Value |
|---|---|
| Model | Intel Core Ultra 7 255H (`GenuineIntel`, family 6, model 197, stepping 2) |
| Microarchitecture | Arrow Lake-H (Core Ultra 200H) |
| Cores / threads | 16 cores, **1 thread per core** (no SMT): 6P + 8E + 2 LP-E |
| Max clock | 5.1 GHz |
| Caches | L1d 480 KiB, L1i 768 KiB, L2 28 MiB, L3 24 MiB |
| Physical address bits | **42** |
| Virtual address bits | 48 |
| Notable flags | `avx`, `avx2`, `fma`, `avx_vnni`, `gfni`, `vaes`, `vpclmulqdq`, `sha_ni`, `movdiri`, `movdir64b`, `fsrm`, `waitpkg`, `pku`, `ibt`, `shstk` |
| **AVX-512** | **absent** (consumer Arrow Lake has no AVX-512) |
| Virtualization | VT-x present |

Stela's stated requirement is x86-64-v2 (Nehalem/Bulldozer or newer), with
AVX2/AVX-512 used when present. This CPU satisfies v2 and v3/v4-class AVX2
paths; Stela must take its non-AVX-512 path here. `42-bit` physical
addressing is relevant: DMA masks and any physical-address widening in the
GPU/IOMMU path must fit 42 bits (xe sets per-platform `dma_mask_size`,
typically 39 for integrated ARL/MTL parts).

## 2. GPU — Intel Arc 140T (Xe-LPG+), the port target

| Fact | Value |
|---|---|
| PCI | `0000:00:02.0`, vendor `8086`, device **`7d51`**, rev `03` |
| Name (Linux) | Arrow Lake-P [Arc Pro 130T/140T] |
| Subsystem | `1043:1fd3` (ASUS) |
| Class | `0300` VGA |
| Kernel driver now | `i915` (module `xe` also available) |
| Graphics IP | **Xe-LPG+**, graphics version **12.74** (upstream `drm/xe/arl`: "ARL-H uses the same media and display IP as MTL and a version 12.74 graphics IP (Xe_LPG+); treat the whole platform as MTL") |
| Mesa (host) | 26.2.5, `Mesa Intel(R) Graphics (ARL)`, GL 4.6 / GLES 3.2, unified memory, 15332 MB reported |
| Vulkan | `vulkaninfo` is **not installed** on this machine — Vulkan status unverified until `vulkan-tools` is installed |
| VRAM | **none** — shared LPDDR5 system memory |
| Display engine | integrated (eDP + DP + HDMI), `intel_backlight` present |
| NPU | `8086:7d1d` (Intel VPU) — separate, irrelevant to Stela |

Upstream support status for `0x7D51`:
- **xe (kernel)**: in the MTL ID set (`XE_MTL_IDS`), added by
  `drm/xe/arl: Add Arrow Lake H support` (Linux 6.9). Stela's xe port builds
  unchanged, so the device is table-covered.
- **ANV (Mesa)**: in ANV's PCI ID list since **Mesa 24.1** (IDs `0x7d41`,
  `0x7d51`, `0x7d67`, `0x7dd1`), reusing Meteor Lake code paths.
- **BRW**: Xe-LPG+ compiles through the same BRW backend as MTL; workarounds
  keyed on `GRAPHICS_VERx100 == 1274`.
- **Stela** (verified against the pinned tree, Linux 7.2.12): the ID is at
  `include/drm/intel/pciids.h:803`, inside `INTEL_ARL_H_IDS`, and
  `drivers/gpu/drm/xe/xe_pci.c:514` maps `INTEL_ARL_IDS(..., &mtl_desc)`.
  Consequences that matter to the port:
  - `xe::admits(0x7d51)` **succeeds** — the port's kernel-driver gate is open.
  - `xe::platform(0x7d51).name` is **`"METEORLAKE"`** (`PLATFORM(METEORLAKE)`
    stringifies into Stela's generated table), `display = true`,
    `discrete = false`. So the Metal device will report architecture
    `METEORLAKE` for an Arrow Lake GPU: upstream's own choice, cosmetically
    wrong, functionally right. Naming ARL separately would mean deviating
    from xe's table in `build/xe.rs`.
  - `dma_mask_size` for this device is **46 bits** (from `mtl_desc`), not a
    narrower integrated mask; BAR/memory mappings must respect that.
  - `mtl_desc.require_force_probe = true` at this pin, so xe only binds ARL
    when force probe is set — Stela's profile already sets
    `CONFIG_DRM_XE_FORCE_PROBE` for every device in xe's table.

## 3. Memory — constrained but workable

| Fact | Value |
|---|---|
| Installed | 16 GB nominal |
| Usable (`free -h`) | **14 GiB total** (firmware/GPU carve-out) |
| At audit time | 13 GiB used, **1.3 GiB available**, 29 GiB zram swap (15 GiB used) |
| Shared with GPU | yes — no dedicated VRAM |

Consequences: a full `cargo xtask dist` (LLVM + Mesa + Linux) is the single
largest risk on this machine; build with `--jobs 2..4`, not 8. Stela's own
`default_jobs()` already caps at 8; that cap is still too high here.

## 4. Storage — **blocker**

| Fact | Value |
|---|---|
| Disk | 476.9 GB NVMe, GPT |
| Partitions | `nvme0n1p1` 2G vfat `/boot`; `nvme0n1p2` 474.9G LUKS→btrfs `/` |
| Controller | Sandisk **WD PC SN5000S** NVMe, `15b7:5036`, **DRAM-less** |
| Path | **behind Intel VMD**: `8086:7d0b` RST VMD, NVMe enumerated at `10000:e1:00.0` (VMD domain), `10000:e0:06.2` root port |

Stela's requirement is "NVMe, or SATA in AHCI mode". On this machine the NVMe
controller is **hidden behind Intel VMD (RST)**. Stela has no VMD support, so
as configured it will see no boot disk. Options, in order of preference:

1. **Install to an external USB/NVMe enclosure** (no firmware change, no risk
   to the Linux install). Recommended for all bring-up work.
2. Switch the BIOS storage mode from RAID/VMD to AHCI — this changes how the
   existing Linux installation sees its disk and can break its boot. **Not
   to be done without the user's explicit consent and a backup.**
3. Add VMD support to Stela — a new PCI domain/bridge model plus a second
   enumeration root. Real work, no upstream code.

## 5. Wi-Fi — not supported by Stela

| Fact | Value |
|---|---|
| PCI | `8086:7740` Arrow Lake CNVi WiFi, driver `iwlwifi` |
| Subsystem | `8086:00e4` (Intel CNVi companion) |

Stela's README: "Wi-Fi: None currently supported." The device is a CNVi part
(host MAC in the chipset, RF companion over CNVi), which cannot be modelled
by simply porting `iwlwifi`; it also needs the firmware recipe and the
CNVi/companion link. **Out of scope until graphics work.**

## 6. Bluetooth — not supported by Stela

| Fact | Value |
|---|---|
| USB | `8087:0037` Intel Corp. (Bluetooth controller, BE200/AX211 class) |

Stela has an Intel BT firmware/stage machine (`crates/stela-drivers/src/bluetooth/intel.rs`)
used for other chips, but Stela supports no Bluetooth transport end-to-end
today. Out of scope.

## 7. Audio — not supported by Stela

| Fact | Value |
|---|---|
| PCI | `8086:7728` HD Audio, `sof-audio-pci-intel-mtl` |
| Codec graph | `sof-hda-dsp`: analog + 3 HDMI/DP outputs (HDA codec driven by SOF on Linux) |

Stela has HDA code (`stela-driver-audio`) but no working audio on any
machine per its README. Note the laptop's speakers are driven through the
SOF/DSP topology plus external amps; macOS audio here is a project of its own.

## 8. Display — reachable, unproven

| Fact | Value |
|---|---|
| Connectors | `eDP-1` **connected** (internal panel), `DP-1`, `DP-2`, `HDMI-A-1` disconnected |
| Backlight | `/sys/class/backlight/intel_backlight` |
| Display engine | Intel display, shared i915/xe display code (Stela compiles it via `build/xe.rs`) |

At least one output exists on the Intel GPU, so the "laptop with no display
mux" copy path described in `gpu_status.rs` is not automatically required —
potentially the panel can be driven directly by xe's display code once the
stack runs. Brightness, refresh and resolution behaviour are unverified on
Stela.

**Brightness has a known blocker.** `intel_backlight` is registered through
`backlight.c`, which finds its device with `class_find_device`; that symbol
is an `__builtin_trap()` stub in Stela's compat layer (no class device list
exists). Registering the backlight would therefore fault, so the driver
core's class/device lists must be implemented before brightness is claimed.

## 9. Summary table

| Component | Stela today | Needed for this laptop | Verdict |
|---|---|---|---|
| CPU (ARL-H, AVX2, no AVX-512) | supported (v2+) | nothing | OK |
| GPU (Arc 140T `0x7d51`) | xe port + ANV admission exist; **ANV not linked, never run** | finish Intel stack | **the project** |
| RAM 16 GB | 8 GB minimum | build/run tuning | tight but OK |
| SSD behind VMD | NVMe only, no VMD | external USB disk or VMD support | **blocker for internal boot** |
| Wi-Fi (CNVi) | unsupported | CNVi model | unsupported |
| Bluetooth | unsupported | new transport | unsupported |
| Audio (SOF) | unsupported | SOF topology + amps | unsupported |
| Display (eDP) | xe display code compiles | bring-up + brightness | unproven |

## Measured: the machine's own PCI census (2026-11-16)

From `boot-log-hardware-2026-11-16.log`, the boot that first ran on the UX3405CA.
Stela enumerates 40 functions (`HOST PCI`). What matters for the port:

| Address | ID | Census line | Meaning |
|---|---|---|---|
| `00:02.0` | `8086:7d51` | `: GPU` + `HOST GPU … xe will bind it` | the Arc 140T; the Intel stack admits it |
| `00:0e.0` | `8086:7d0b` | `: Intel VMD storage` | the VMD that hides the internal NVMe |
| `00:0d.0` | `8086:7ec0` | `: xHCI USB` | failed to attach (see below) |
| `00:14.0` | `8086:777d` | `: xHCI USB` | second controller |
| `00:0d.2` | `8086:7ec2` | `: USB4/Thunderbolt NHI` | not driven |
| `00:14.3` | `8086:7740` | `: iwlwifi Wi-Fi (with --wifi)` | opt-in only |
| `00:1f.3` | `8086:7728` | `: not driven` | SOF audio |
| `00:1f.4` | `8086:7722` | `: i801 SMBus` | driven |

Display: real EDID, `1920x1200` at `60.003 Hz`, panel `302x189 mm`. That is the
panel's native mode, so the GOP path works on this machine without any stub.

USB is the current obstacle: `HOST DEVICE FAILED pci-0000:00:0d.0 (USB)
error=native resource service unavailable`. The controller never attaches, so
Stela cannot read its own stick through its drivers after ExitBootServices, and
the on-stick boot log stops at that boundary.
