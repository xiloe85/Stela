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

## 1a. Linux starts and an install completes, but the GPU start's own report is not on the stick

Boots 7 and 8 (2026-11-17) stopped after seven `LINUX` lines, the last from
inside `workingset_init()`, with no panic and no watchdog stop: the two
completions PID 1 passes had no timeout and ran inside one dispatch step, so the
loop that reports a stuck step could not speak.

Boot 9, carrying the image that bounds and names those boundaries, is past them
and reports no failure at all:

    LINUX <6> stela: built-in initcalls: workingset done (0)
    LINUX <6> stela: built-in initcalls done; resource roots complete
    LINUX <6> stela: threads: supervisor claimed pid 1
    LINUX <6> stela: threads: kthreadd claimed pid 2
    LINUX <6> stela-init: kthreadd entered; starting the workqueues

The macOS installer then ran for 1310 s and its checkpoint protocol completed
(`seal_system_volume`, `create_system_snapshot`, `set_autoboot_delay`, the
cleanups, `AMRError 0`), writing 26.7 GiB to the internal SSD and asking for a
restart.

What is still unmeasured is the probe. Stela holds the host drivers' lock for
Linux's whole start and refuses to store the USB's log from inside a driver
call — the log says so itself (`BOOT LOG not stored: this processor stopped
inside the host drivers`, t=399.478) — so `GPU START claim_ms=… linux_ms=…
probe_ms=…` and the GPU step's own completion line are not in the file. Nothing
here should be read as "xe probed": a failed start would have ended the boot
with `macOS cannot run without a GPU` and none did, which bounds the start at
"it returned success", not at "DRM is bound" — and that reading is now known to
be wrong as well: a failed start does not stop the boot at all, since
`open_gpus` returning false only skips `gpu::serve`. What is certain is that
nothing was ever presented (`CURSOR moved_per_wall_second=0.0`, `samples=0`).
The start's own report is now printed inside the start's scope and stored from
there — and boots 9 and 10 both end the stick's record at those same seven
`LINUX` lines, which is a limit of the log rather than a verdict on the start.
The store does work inside the start (boot 10 kept the claim line and those
seven lines, written by `GpuViews::report`); what is missing is anything left to
print. Boot 10's `wait_for_child` reports never appear at all, so in 17 minutes
the start had not reached either wait loop: the time is inside `start_initial`
→ `environment().start(...)`, one call that nothing in this tree bounds.
`START_SECONDS` (60 s) is checked only between steps in `wait_for_child`, and
the 30 s step watchdog fires only when a single step fails to return, so a Linux
start that spins inside one call holds the boot for as long as it spins (boot 9:
at least 742 s of it; boot 10: not finished in 1020 s). `stage_mark` brackets
those calls now, so the next boot times them (`GPU START stage=linux-initial
done ms=…`) instead of leaving a cliff.

Boot 11 (2026-11-18, same image, left alone for an hour) answered that and
moved the cliff 100 ms later: `stage=linux-initial ms=24` → `done ms=28`, so
the call boot 10 never returned from took **4 ms**; `srcu` took 7 ms; Linux's
first tasks ran; and `stage=probe ms=76` is the last line the stick holds. The
same image both stalled for 17+ minutes inside `start_initial` and passed it in
4 ms, so this is a race in Linux's task start that moves between calls, not a
slow path. Because `gpu::open` must return within `START_SECONDS` (60 s) and
`start.rs` prints and stores `GPU START claim_ms=… linux_ms=… probe_ms=…
total_ms=…` when it does, that line's absence says the start never returned:
one call after the probe mark — `threads.submit(job)` or the first
`threads.dispatcher_step()` — did not come back. The stop's reason is not on
the stick (a stop inside the host drivers cannot store it); it is kept in the
PC's firmware (`StelaStopHeader`, GUID
`b3e1f6a4-2c5d-4e8f-9a17-6d0c53a7e142`) for the next Stela start to copy into
the log, and Linux's efivarfs does not expose that GUID, so it takes one more
start to read it.
**The screen goes black exactly when the claim succeeds, and boot 9 shows the
mechanism.** `display.rs` starts from this premise: binding a GPU ends the
firmware's framebuffer on that function, so Stela's console moves to the
output's *front buffer* through the driver's KMS, where the modeled DCP's frames
follow it, and "a GPU driving no display leaves the firmware's scanout alone".
The claim reset the function (FLR) and no display was ever taken — the whole
boot reported `CURSOR moved_per_wall_second=0.0` with `samples=0` — so there was
no front buffer to move to and the panel stayed dark through 22 minutes of
install. The fix is not a framebuffer copy: the display has to be *taken* (a
KMS display bound on the Intel display engine, as `display.rs` expects) or the
firmware's scanout has to be re-programmed after the reset. Segments found and what each would need are in `DEVELOPMENT_LOG.md`, session 10 continued.

Boot 12 (2026-11-18) put the two halves of this on one record. The header
reads `DISPLAY firmware mode 1920x1200 Bgr` / `DISPLAY runs 1920x1200 Bgr:
the panel's native resolution`, so the firmware's pipe is lit and the boot
paints its step list into it; the claim's `reset by Function Level Reset`
comes 86 s later and nothing has taken the pipe, which is the moment the
panel goes dark. Separately, that boot's marks show `GPU START stage=submit
done ms=76` and `wait-step stage=the GPU's probe ms=0` followed by nothing:
the probe was handed to Linux's handler and never returned, so even a
successful later step could not re-light the panel. The two are independent —
keeping the firmware's pipe (no FLR on the display function, which is what
the Arrow Lake laptops that run a desktop do) fixes the black screen, and the
probe's stall needs its own answer.


## 2. Intel VMD must stay off in the firmware (it is, and the SSD works)

This used to read as a blocker: `8086:7d0b` RST VMD active puts the NVMe
controller (`15b7:5036`, WD PC SN5000S) at `10000:e1:00.0` inside the VMD
domain, and Stela supports NVMe directly, not VMD. VMD is now disabled in the
firmware, and boot 9 proves the disk is Stela's: the controller starts as
`0000:01:00.0 id=15b7:5036`, its partition `9C6963CC-C904-4BDA-BFA7-A67446C5466D`
(171,460,608 blocks) is found as `(installed system)`, and 28,019,160 KiB were
written to it during the install. If VMD is ever switched back on, this becomes
a blocker again, and the mitigations are an external USB/NVMe disk, AHCI mode
(which would affect the installed Linux), or implementing VMD (substantial).

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

## `--host-check` cannot bring devices up (by construction)

The host check runs `stela_drivers::probe::probe()`, the legacy entry point, which
passes no native mount. `minimum_result` is then
`Err(ServiceError::Unavailable)`, so every driver that needs the mount's
minimum-delay timer, BAR ownership or IRQ routing refuses before it starts:
xHCI, NVMe, virtio. On real hardware this showed as
`HOST DEVICE FAILED pci=0000:00:0d.0 (USB) error=native resource service
unavailable`.

Consequences, which the check now states instead of failing:

- `storage`, `gpt` and `keyboard` are `SKIP` in this lane. They are only real
  verdicts under the system engine, which builds the native mount.
- The on-stick boot log stops at ExitBootServices in this lane: continuing it
  needs Stela's own block driver over the very USB controller the lane cannot
  start.
- A GPU verdict still works, because the census reads configuration space and
  selection (`HOST GPU … xe will bind it`) needs no mount.

## The disk-check lane cannot pass in this tree

`--storage-check` (boot stage, `crates/stela-boot/src/storage.rs::Prepared::run`)
calls `probe()` without a native mount, so every disk driver fails with
`native resource service unavailable` and the run stops at
`STORAGE REFUSED …`. The engine's own disk path needs a real mount
(`Services::new` over `native_mmio::Source::activate` and `Remappers::take`) and
the `system-kernel` board model exposes no ANS2, so a disk cannot be attached
there either. Until one of those is built, disk bring-up is only proven by booting
a real system, not in QEMU.
