# KNOWN LIMITATIONS

Honest inventory of what is **not** working, with the reason and the way
forward. Nothing here is hidden behind optimism.

## 1. No Intel execution has happened yet

`GpuStack::INTEL.vulkan` is `None` in upstream Stela. ANV is admitted by the
Metal layer and built into the image's driver list, but it has **never run on
an Intel GPU under Stela**. Upstream states this in the code comment:
"ANV has run on no Intel GPU under Stela." Therefore:

- Level 4 (a real GPU computation) — **not achieved**.
- Level 5 (a Metal shader through the Intel backend) — **not achieved**.
- Levels 6–10 — **not achieved**.

## 2. The internal SSD is behind Intel VMD (blocker for installing macOS)

Verified on the machine: `8086:7d0b` RST VMD is active, the NVMe controller
(`15b7:5036`, WD PC SN5000S) appears at `10000:e1:00.0` inside the VMD
domain. Stela supports NVMe directly, not VMD. As configured, Stela will not
see this machine's internal disk. Mitigations, in order: use an external
USB/NVMe disk (recommended, zero risk); or change the firmware storage mode
to AHCI (affects the installed Linux system — requires the user's explicit
consent); or implement VMD in Stela (substantial new work).

## 3. 16 GB of shared memory

14 GiB is usable, the GPU shares it, and this machine currently runs with
~1 GiB free. Building the image (LLVM + Mesa + Linux) and running macOS plus
a GPU driver in the same RAM is the tightest resource here. Practical
consequences: build with `--jobs 2..4`; expect shader-cache pressure; a
16 GB machine can boot but "stable accelerated desktop" will need cache
caps and careful buffer lifetimes.

## 3a. Brightness/backlight will fault, not fail gracefully

`class_find_device` and `device_find_child` are `STELA_UNSUPPORTED` traps
(`__builtin_trap()`) in Stela's compat layer, because the driver core keeps
no class device list. `backlight.c` and `hwmon.c` use exactly those searches
for a panel's backlight and the GT's sensors. On an Intel laptop with an
internal panel this is reachable, so **brightness is blocked on implementing
the driver core's class and device device lists**, and until then a modeset
that registers the backlight can fault instead of reporting an error. The
build's import list now declares them for xe (required to build the Intel
image at all), with that warning beside it.

## 4. Firmware, display and submission bring-up is unfinished upstream

- GuC/HuC/GSC/DMC firmware recipes exist, but only nouveau's claim path
  currently requests firmware; there is no Intel caller.
- The VBT (OpRegion) reader exists with no caller.
- xe's display code compiles but no modeset has been attempted under Stela.
- `stela-boot` does not enable `xe-port`; only `nouveau-port` is requested.

## 5. macOS 27 specifics

Stela models an M2 Mac mini (Apple7). Capabilities are claimed as Apple1–7
with ray tracing refused, Metal 4 and Apple8+ refused. Whether macOS 27
accepts a translated arm64e environment on this hardware is **untested**;
nothing in this audit changes that risk, and no attempt was made to boot
macOS (no boot media was created; no disk was touched).

## 6. Wi-Fi, Bluetooth, audio

Unsupported by Stela on **any** machine today. This laptop's parts:
Wi-Fi `8086:7740` (Arrow Lake CNVi — needs a CNVi host+companion model, not
a simple iwlwifi port), Bluetooth `8087:0037` (USB), audio `8086:7728` with
SOF topology plus speaker amps. Each is a project in itself; none is on the
critical path to a GPU-accelerated desktop.

## 7. No AVX-512 on this CPU

The Core Ultra 7 255H has no AVX-512. Stela uses AVX2 where available and
must not require AVX-512; this is a performance consideration for the
translator, not a correctness one.

## 8. Verification gaps in this pass

Recorded so they are not mistaken for successes:

- Full image build was not completed (sparse submodule fetch + hours of
  compile on a memory-tight machine). **Level 1 is therefore not yet
  demonstrated on this hardware.**
- `vulkaninfo` is not installed, so the *Vulkan* capability set of this GPU
  on this host is unverified (OpenGL 4.6 through Mesa 26.2.5 is verified).
- No macOS boot was attempted, and no USB installer was written.

## 9. What is genuinely reusable / already done

To keep the picture balanced: the translator, the Metal layer, AIR→NIR, the
xe port with shared i915 display, the ANV build rules, the Intel admission
and the capability gate all already exist. The gap is integration and
bring-up on real hardware, not the absence of an architecture.
