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

---

## 2026-11-16 — Session 3: Level 1 on this machine, and Intel in the image

### Level 1: `stela.efi` builds here

Two host gaps had to be closed first:

1. Mesa's Rust cross build needs `std` for `x86_64-unknown-linux-musl`;
   Arch's `rustc` ships only the native target. Installed **rustup**
   user-locally (`--no-modify-path`, nothing in the shell config) and
   `rustup toolchain install 1.99.1 --component rustfmt,clippy,llvm-tools
   --target x86_64-unknown-linux-musl,x86_64-unknown-uefi`, matching the
   repository's own `rust-toolchain.toml`. Arch has no `rust-uefi` package,
   so rustup is the only route to the image target.
2. Builds then run with `~/.cargo/bin` first on `PATH`.

```
cargo xtask gpu nvk --jobs 3
  -> libstela-llvm.a 31.7 MB, libstela-nvk.a 44.2 MB (1257 objects of
     "nouveau, intel, amd"), libstela-vulkan.a 21.7 MB, StelaGPU.kext
cargo xtask boot build --release
  -> stela.efi 158,515,200 bytes, stela-deploy.efi 50,849,280 bytes
     PE32+ executable for EFI (application), x86-64
```

### Intel in the image

`xtask/src/boot.rs` gains `STELA_XE` (the tree's existing env-switch
convention, as `STELA_APP` is): set, the image is built with
`stela-boot`'s `xe` feature, which pulls `stela-drivers/xe-port`.

```
STELA_XE=1 cargo xtask boot build --release
  -> stela.efi 204,364,288 bytes, stela-deploy.efi 52,912,640 bytes
```

The growth is the driver: the image's newest build directory holds
`captured-sgpugxeu1n33.o` (xe's registration unit at ordinal 33),
`libstela-xe.a` and **1109 objects against 860** for the NVIDIA-only image,
including `xe_reg_whitelist.o`, `xe_reg_sr.o` and `xe_ras.o`.

What that does and does not mean: `stela.efi` now contains xe with the
i915 display code shared with it, the Intel Metal admission and ANV's ICD
entry (the archive lists `nouveau, intel, amd`, so `stela_vulkan_intel` is
set), and the NVIDIA path is unchanged. **No boot has run this image**, on
Intel or otherwise; nothing here shows a GPU initialised.

### Commits

See `git log` on `fork` — the xtask switch and this log.

---

## 2026-11-16 — Session 2: host tools, real ANV data, parallel agents

### Host packages installed (with the user's approval, elevation requested)

```
pacman -S --needed --noconfirm spirv-llvm-translator vulkan-tools
  spirv-llvm-translator 23.1.2-1, vulkan-tools 1.4.363.0-1  (nothing else pulled)
```

### This GPU's Vulkan, measured

`vulkaninfo --summary` (saved as `vulkan-arl-140t.txt`): **Vulkan 1.4.354**,
`Intel open-source Mesa driver`, `Intel(R) Graphics (ARL)`, `INTEGRATED_GPU`,
Mesa 26.2.5. **218 device extensions.**

### ANV host test: passed, including this laptop's GPU

```
Arrow Lake P: ANV on xe: Intel(R) Graphics (ARL) (0x7d51, integrated),
  NIR options for 6 stages, compute pipeline of 12 instructions in 208 bytes,
  one dispatch submitted
  xe ioctls: DEVICE_QUERY 19, EXEC 4, EXEC_QUEUE_CREATE 2, EXEC_QUEUE_DESTROY 2,
    EXEC_QUEUE_GET_PROPERTY 2, GEM_CREATE 13, GEM_MMAP_OFFSET 14, VM_BIND 28,
    VM_CREATE 1, VM_DESTROY 1
ANV ran on xe's ioctls for 7 platforms, with no GPU
```

Everything ANV issued is an ioctl Stela's xe registers. **No GPU behind the
shim**, so this proves device creation, BRW codegen for ARL and Stela's
submission interface — not silicon execution.

### The extension question, settled with data

30 of the 33 extensions Stela enables are present in ANV here. Missing:
`VK_KHR_unified_image_layouts` (performance only: Stela uses
`VK_IMAGE_LAYOUT_GENERAL` for every image and only ever transitions
GENERAL→GENERAL, which core Vulkan allows), `VK_KHR_fragment_shader_barycentric`
(Metal `BARYCENTRICS` must stay refused) and
`VK_NV_shader_atomic_float16_vector` (NVIDIA-only). Details in `ANV_GAPS.md`.
Since the vendored Mesa has the unified-layout extension in NVK and RADV but
not ANV, Intel will be slower on texture-heavy work, not blocked.

### Parallel agents (opencode, `opencode/glm-5.3-flash-free`)

Three agents ran on disjoint scopes: driver-core device lists (code), ANV gap
analysis (read-only), Intel firmware wiring (code + plan). **All three hit the
free-tier usage limit mid-run** (`Error: Free usage limit reached`), so their
work is partial and was verified by hand rather than trusted:

| Scope | Landed | Verified how |
|---|---|---|
| driver-core `class_find_device` / `device_find_child` | real implementations over a membership list, add/del hooks, reference held on return, plus trap removal from nouveau/amdgpu/xe `OPEN` lists and assertions in `device.c`'s hosted test body | `cargo test -p stela-driver-gpu --features xe-port,metal` → **654 passed** (image compiles, import guard accepts the supplied symbols). The new C assertions sit behind `STELA_LINUX_DEVICE_TEST` and were **not executed** by the runs here |
| ANV gap analysis | agent produced nothing; written by hand from `vulkaninfo` + code reads | `ANV_GAPS.md` |
| Intel firmware wiring | `recipes::XE` family added to `bundle::FAMILIES` and `select()`, opt-in `xe` feature in `stela-boot`, an ARL firmware-file test; bundle totals updated | `cargo test -p stela-loader --lib` → **355 passed** |

Lesson recorded for the next session: brief agents with the verification
command they must run, expect them to stop on quota, and never commit their
diff unread.

### Not done in this session

- `cargo xtask gpu nvk` / `dist` (image build): not completed.
- `cargo xtask gpu anv-test`: not run (fetch incomplete; also needs
  `spirv-tools` + `spirv-llvm-translator` on the host).
- No macOS boot attempt; no USB media written; no disk touched.

## Session 4 — first real-hardware boot (2026-11-16)

Image: Intel + diagnostics (`STELA_XE=1 cargo xtask boot build --release
--diagnostics`) → `stela.efi` 205,270,016 B, `stela-deploy.efi` 53,819,392 B,
written to a 14.9 GB GPT/FAT32 stick with `--host-check --deadline 120` and a
preallocated `\EFI\stela\boot.log` (layout in `BOOT_QEMU.md`). Booted on the
UX3405CA itself. Raw log: `boot-log-hardware-2026-11-16.log`.

### What the firmware phase proves

- Stela mounts, connects 554 of 561 firmware handles, reads settings, finds the
  real panel (1920x1200 at 60.003 Hz, EDID 302x189 mm) and snapshots the image.
- The PCI census runs on real hardware and lists 40 functions, including
  `00:02.0 8086:7d51 … : GPU`, `00:0e.0 8086:7d0b : Intel VMD storage`,
  `00:0d.0 8086:7ec0 : xHCI USB`, `00:14.0 8086:777d : xHCI USB`,
  `00:14.3 8086:7740 : iwlwifi`, `00:1f.3 8086:7728 : not driven` (audio),
  `00:1f.4 8086:7722 : i801 SMBus`, `00:0d.2 8086:7ec2 : USB4/Thunderbolt NHI`.
- **`HOST GPU 0000:00:02.0 8086:7d51: xe will bind it`** — the first evidence on
  the real device that the Intel stack admits the Arc 140T. Precisely: the
  admission test (xe's PCI ID tables plus ARL→Meteor Lake platform) answers yes;
  nothing has probed or bound the GPU yet.
- CPU: x86-64-v2, invariant TSC 3.6864 GHz from a 38.4 MHz crystal
  (`leaf 0x15 tsc/crystal=192/2`), base 3700 / max 5100 MHz, no AVX-512, and the
  firmware reports 42-bit physical / 48-bit virtual addresses (no LA57).

### Where it stopped, and why the log stops there

Screen: `CHECK native-memory PASS`, then
`HOST DEVICE FAILED pci-0000:00:0d.0 (USB) error=native resource service
unavailable`. The file ends at 22.542 s on the GPU line — the
ExitBootServices boundary. One cause for both: after EBS the boot log is written
through Stela's own block driver over the same USB stick, and the xHCI controller
that stick hangs on failed to attach, so the engine never got the volume back and
could not keep writing. The console continued because it has its own path.

`native resource service unavailable` is `ServiceError::Unavailable`
(`crates/stela-drivers/src/probe/services.rs:74`): the mount supplies no such
service for that device. Prime suspect is `Services::interrupts()`
(`crates/stela-boot/src/engine/services.rs:397`), which is `Unavailable` until
`native_irq::start` has mounted the boot CPU's identity — a system-engine startup
step `--host-check` does not perform (it installs its own IDT and APIC instead).
The QEMU runs showed the same storage/keyboard failures, which fits a
**host-check lane** limitation rather than a hardware defect. The exact xHCI call
site is still open: the two known `services.interrupts()` callers are `bcm4377`
and `intel_bluetooth`, neither of them USB.

### Not proven by this run

- No xe probe, no firmware load, no GuC/HuC/GSC claim, no modeset. "xe will bind
  it" is the selection answer, not a bound device.
- Storage, GPT, keyboard and interrupt checks reached no verdict on hardware, and
  the log could not be kept past EBS.

### Session 4 addendum — the USB failure was the lane, not the device

Traced `HOST DEVICE FAILED pci=0000:00:0d.0 (USB) error=native resource service
unavailable` to its source: `stela_drivers::probe::probe()` (the legacy entry
point, `crates/stela-drivers/src/probe.rs:1256`) calls `probe_with_provider(…,
None, None)`, so `services` is `None`; `minimum_result` is therefore
`Err(ServiceError::Unavailable)` (`:1676`), and `minimum_for_function!` (`:1769`)
files that error for every driver that needs the mount's minimum-delay timer —
xHCI, NVMe and virtio alike. The `(USB)` label comes from the function's PCI
class, not from the USB driver. Nothing was wrong with the controller, the stick
or the hardware: `--host-check` never gives device bring-up a mount, so it could
never have started them, and the on-stick log can never continue past
ExitBootServices in that lane.

Fixed the reporting rather than the lane: `host_check::disks` now reads
`Devices::native_services`, prints one explaining line
(`HOST DEVICES no bring-up: …`), does not file those as device failures, and
records `storage`, `gpt` and `keyboard` as `SKIP` with reasons instead of `FAIL`.
The system engine, which builds the real mount, is unaffected.

Verified in QEMU (same image, `-cpu max`, GPT test disk, `--host-check
--deadline 120`):

```
HOST DEVICES no bring-up: this check runs without the native mount, so the drivers that need its minimum-delay timer or BAR ownership do not start
CHECK storage SKIP no native mount: device bring-up is the system engine's
CHECK gpt SKIP no native mount: no internal disk is brought up here
CHECK keyboard SKIP no native mount: no USB controller is started here
CHECK interrupts PASS one_shot=1 after 10939 us (expected 1 after 10000) periodic=100 in 100 ms
HOST CHECK END pass=6 fail=0 skip=5
```

Before the change the same lane reported `pass=6 fail=3 skip=2` with three
`HOST DEVICE FAILED` lines and no explanation.

## Session 5 — the mount's timer, and what it proves about the laptop

### A: the disk-check lane

`cargo xtask boot run --storage-check` (guestless system mode, GPT disks on two
virtio-blk, NVMe and AHCI) aborts before any disk:

```
HOST DEVICE FAILED pci=0000:00:01.0 (storage) error=native resource service unavailable
STORAGE REFUSED stela-virtio:… no disk has serial "stela-virtio"; no disk was found
STELA FATAL storage target refused
```

Same root cause as the hardware run (`probe()` with `services = None` in
`crates/stela-boot/src/storage.rs::Prepared::run`), not Intel-specific. `--iommu
intel` changes nothing. Fixing it means building a real mount outside the system
engine (`Services::new` needs `native_mmio::Source::activate` and
`Remappers::take`, both engine-owned), so it was left as found and recorded.

### A: the engine lane does work, and shows a timer requirement

The engine path (`--scenario system-kernel`, which boots the test kernel with the
real mount) fails differently, and this is the useful finding:

```
HOST DEVICE FAILED pci=0000:00:01.0 (storage) error=minimum delay provider: Unavailable
```

That is `ServiceError::MinimumDelay`, so the mount *is* present here; what is
missing is its timer. `Services::minimum_delay` takes the TSC only when
`INVARIANT_TSC` is present, else falls back to the claimed HPET/PM timer, and
nothing installed one. QEMU under TCG does not advertise an invariant TSC —
under `--kvm` (host CPU) it does, and then:

```
ENTRY pc=0x800004000 … el=2 … profile=stela-test-kernel target=x86_64-baseline+ssse3+sse4.1+…
GUEST| STELA TEST KERNEL: MMU ON, UART MMIO
STOP idle: every PE is off or waits with no wake source
```

So the system engine, the native platform and guest execution all work with the
Intel diagnostics image on this host. The real machine reports
`invariant-TSC` and `source=Cpuid` (session 4), so the timer path that blocks TCG
is exactly the path the laptop satisfies — a good omen for the hardware run.

Still not exercised: the engine's disk bring-up. `--storage SERIAL:GUID` with
`--scenario system-kernel` refuses with `this DeviceTree exposes no modeled
ANS2`: that scenario's board model has no ANS2 node, so a disk cannot be attached
to it. Nothing on the host side was changed for this.

Note on exit codes: the run ended with `STOP idle` and `STELA STOPPED code=0x10`
but xtask returned 124 (its wait expired) because the guestless/debug image does
not drive the host exit port under `--kvm`; the serial result is unambiguous.

## Session 5 — the installer artifact (what Stela actually consumes)

Not an `InstallAssistant.pkg`: the host CLI takes an **IPSW** and builds the boot
bundle from it (`stela prepare --ipsw … --model Mac14,3 --output …`), or writes a
whole installer stick (`stela create <stick> --ipsw … --installer offline|online`).

- The emulated identity is **Mac14,3** (M2 Pro Mac mini); the tree references
  kernel build **26A434** 520 times and 26B123 66 times, so the matching image is
  **macOS 27.0.1, build 26A434**.
- Source: Apple's own CDN,
  `updates.cdn-apple.com/2026FallFCS/…/UniversalMac_27.0.1_26A434_Restore.ipsw`,
  26,637,307,067 bytes, `signed=true`.
- Downloaded and checked against Apple's published digest:
  `2f016638293c3e641b8b25391a76fbc16563b3711915a5551cf8aa0f5598a5c1` — matches.
- Installer modes (`crates/stela-cli/src/setup.rs`): **Offline** copies the whole
  IPSW onto the stick and needs ≥32 GB (`FULL_MIN`); **Online** keeps the strip
  small and downloads macOS from Apple during the install, so the installer needs
  a network — `--wifi SSID` (passphrase from a file) or Ethernet.
- The 14.9 GB Cruzer Blade therefore cannot hold an Offline installer; it was dry
  run as Online: "Would erase sda (Cruzer Blade) and make an Online installer on
  it."

### Session 5 — the installer stick, built and verified

`stela create /dev/sda --installer online --ipsw …` needs the CLI from
`cargo xtask dist` (the plain `cargo build -p stela-cli` refuses: "this stela was
built without its boot image"). Dist embeds `stela-dist.efi` — 52,926,976 B with
`STELA_XE=1` — into a 65,341,648-byte `stela`. The first `create` was killed
mid-run (2.2 GB written, EFI volume empty); `stela repair /dev/sda --ipsw …`
finished it: boot files 507.3 MB, firmware 189.1 MiB, settings restored.

`stela status /dev/sda --full` (every byte read back):

```
model          Mac14,3      Stela 0.4.2      macOS 27.0.1 (26A434)
Wi-Fi          none: an Online install needs Ethernet
firmware       complete (189.1 MiB)
boot files     intact
install        Online: the install downloads macOS, so the PC needs internet
No problems found.
```

So the stick carries an Online installer: the macOS payload is not on it, the
install fetches it. Wi-Fi is still unset (`stela settings` can add it without
rebuilding); otherwise the PC needs Ethernet in the installer.

### Session 5 — making room on the internal NVMe, without buying anything

The install target problem: no external disk. Stela's picker can install into
**free space** (`installer.rs`: "macOS can take free space or a Stela partition
already there… Filesystems are never resized: another system's space is freed
only by deleting its partition"), so the internal 512 GB WD PC SN5000S can host
macOS beside Omarchy if 120 GB is left unallocated. Minimum is 60 GB
(`DEFAULT_MIN_APPLE_BYTES == 60 * GB`).

Machine: `nvme0n1` 1,000,215,216 sectors; p1 ESP 2 GiB (unchanged); p2 LUKS2
996,016,128 sectors (509,960,257,536 B) holding btrfs `root` (474.92 GiB, 235.66
GiB used, device mapper 509,943,480,320 B = p2 − 32,768 sectors of LUKS2 header).

Safety net first, in `~/Downloads/shrink-safety/`: `luks-header-nvme0n1p2.img`
(16 MiB), `gpt-nvme0n1.sf` (text dump), `gpt-head-34s.bin`, `gpt-tail-33s.bin`,
a copy of `/boot`, and `check_gpt.py`, which validates both GPT copies by
recomputing the header and entry-array CRC32s and comparing them. `btrfs scrub`
before touching anything: 228.73 GiB at 2.29 GiB/s, **no errors found**; device
stats all zero.

The shrink, in three steps:

1. `btrfs filesystem resize 389944442880 /` — online, exit 0, 474.92 GiB →
   363.16 GiB, 123.5 GB left free inside the filesystem.
2. LUKS: **not resized live.** `cryptsetup resize --size …` demands the
   passphrase in 2.8.8 even with `-q --batch-mode` (debug: "Checking volume
   passphrase … STDIN descriptor passphrase entry requested"), because shrinking
   re-activates the mapping. Unnecessary anyway: the mapping size is derived
   from the partition (`size = p2 − 32,768`), so trimming p2 makes the next
   activation land on exactly the filesystem's size. Verified by arithmetic:
   761,643,008 − 32,768 = 761,610,240 sectors = 389,944,442,880 B.
3. `sfdisk --no-reread --force` with p2 resized to 761,643,008 sectors (start
   unchanged at 4,196,352, PARTUUID unchanged). Kernel initially kept the old
   table ("Device or resource busy"), both on-disk copies verified by CRC and
   agreeing; the free gap is 234,375,823 sectors = 120.0004 GB, starting on a
   2048-sector boundary. macOS takes 120 GB, Linux keeps 363 GiB with ~123 GB
   free.

Verified after the reboot: `nvme0n1p2` 389,961,220,096 B, `/dev/mapper/root`
389,944,442,880 B (exactly the filesystem size, the mapping derived from the
partition as predicted), `/` 364 G with 116 G available, both GPT copies still
CRC-clean and agreeing, and the system boots normally. The LUKS resize step was
not needed; the derived size is what made that safe.

## Session 6: an install with no network — the macOS payload on the internal disk

### What stopped the install

VMD was the disk; the network is the next wall. With VMD disabled in firmware,
`Preparing the chosen disk for macOS` passed and Stela created the Apple
partition (`p3`, 120.000 GB) — so the disk path works. The install then stopped
one step later:

    Network adapter not started: 0000:80:19.6 8086:7740 Wi-Fi: PCI configuration:
    bus mastering stops until the device is taken
    [warn] Stela will not start the network: no supported network adapter is visible

This laptop has no Ethernet port and its radio is Intel `8086:7740`, which
Stela's network stack does not drive. More importantly the radio is the wrong
kind for the target: **macOS has no Intel Wi-Fi driver** — real Macs and
hackintoshes use Broadcom — so no amount of Stela-side work would let Wi-Fi
carry a macOS install. Ethernet or a local payload are the only routes, and
this machine has no Ethernet.

### The route taken: the IPSW on the internal NVMe

The engine does not care where the install medium lives. `--install-source` is a
partition GUID: `bundle::with_inventory` reads `payload.json` through the
firmware's FAT driver on that partition's volume, and `engine::install::open`
reads the pieces through Stela's own FAT32 reader on the same partition. So the
26.6 GB image already downloaded and verified on this machine can serve as the
medium — no purchase, no dongle.

The signing side turned out to be free too: the stick already carries Apple's
staged answers (`stela/boot/26A434/restore-…/tss-answers.plist`, 24,935 bytes),
written when it was created with internet. With the medium local,
`tss::staged::Use::of(Some(set), update=false, remote=false)` is `Offline`, so the
engine opens no network at all (`INSTALL OFFLINE`).

### What changed on the machine

The 120 GB gap was split so a payload partition and an install target can
coexist. `p1` (ESP) and `p2` (LUKS/btrfs) were not touched; only the empty `p3`
that Stela had created was replaced, with both GPT copies CRC-verified after:

| partition | start LBA | sectors | size | use |
|---|---|---|---|---|
| p1 | 2,048 | 4,194,304 | 2.147 GB | Limine ESP (unchanged) |
| p2 | 4,196,352 | 761,643,008 | 389.961 GB | LUKS root (unchanged) |
| p3 | 765,839,360 | 62,914,560 | 32.212 GB | FAT32 `STELAPAY`, GUID `cc24a192-1aa2-415f-9de1-d8262bb84a09` |
| free | 828,753,920 | 171,460,608 | 87.79 GB | the macOS install target |

The medium was written with the tool that already existed for this job, whose
own help says it writes "the install medium `stela setup --target` puts on the
USB … to a mounted FAT32 volume":

    stela-dev install-medium --ipsw UniversalMac_27.0.1_26A434_Restore.ipsw \
        --volume /mnt/stelapayload

    0 pieces kept, 7 written; payload.json SHA-256 2c3092bb…7a67
    Engine options: --install \stela\install\26A434\payload.json --install-source <GUID>

Seven pieces (`ipsw.000`–`ipsw.006`), `pieces.bin` (13.0 MB), `blocks.bin`
(11.6 MB) and a `pieces`-layout `payload.json`; the pieces sum to
26,637,307,067 bytes, exactly the IPSW. No CLI code was needed — the dev command
and the medium writer were already in the tree, so this session changed data,
not source.

### The stick now boots the install offline

`choices.cfg`'s Install macOS line gained `--install-source
cc24a192-1aa2-415f-9de1-d8262bb84a09`, and `setup.json`'s medium record became
local (`partition_guid` set, `remote: false`, `manifest_sha256 2c3092bb…`,
`install_image: "full"`). `stela status /dev/sda --full` then reports:

    macOS copy     complete
    install        Offline: the install needs no internet.

with one expected complaint — "the startup choices differ from the stick's
setup" — because `choices.cfg` was edited rather than regenerated. **`stela
repair` must not be run on this stick**: it would rewrite the choices from the
record's default and reintroduce the network medium.

### Still unproven

Everything up to the engine reading the medium is now verified from Linux: the
GUID, the layout, the piece sizes, the manifest hash, and that the staged
answers are on the stick. What is not yet demonstrated is the engine finding the
partition through the firmware on this machine and the restore running with no
network — that is the next boot. The `--development-restore` and 87.79 GB free
space both satisfy the installer's minimum (`DEFAULT_MIN_APPLE_BYTES` = 60 GB).

### The phantom NVIDIA line, again

`All NVIDIA graphics card found: macOS can be installed, but it starts only with
a GeForce GTX 16 …` is printed on a machine whose only GPU is Intel. It is
cosmetic, it misleads every reader of a log, and it is still unexplained; it
belongs to the GPU census's message path, not to this session's work.

A later photograph at full size settles what the line actually says:
`No NVIDIA graphics card found: macOS can be installed, but it starts only
with a GeForce GTX 15 or RTX 20, 30, 40 or 50 series card.` It is not garbled
output — it is the NVIDIA refusal's text, printed on every machine without an
NVIDIA GPU, and it was misread from an earlier, smaller photograph. The wording
is still wrong for an Intel machine (it claims macOS "can be installed" while
implying only NVIDIA starts it), so it stays on the list as a message bug, not a
translation bug.

## Session 7: the install completes, and the GPU step is the wall

### What the hardware did

With VMD off and the payload on `p3`, the stick's install boot got past every
step that failed before. The `270 s` stall in `Taking control of the other
processors` was transient: the same step took `30 s` on the next boot and
`36 s` earlier, and no `NATIVE AP startup failed` line was ever printed. The
install then ran to completion and restarted the machine on its own, and the
table's count moved from five partitions to six (`Starting the PC's disks and
USB devices (6 partition, 3 s)`).

That reboot is the milestone: macOS 27.0.1 is on the internal NVMe, installed
with no network, leaving `p1` and `p2` untouched.

### Where it stops now

The next boot's screen names the difference in one line:
`Starting macOS` instead of `Starting the install media`. It then stops on:

    [ ] Starting the GPU

and never returns — over ten minutes, with no warning and no fatal message.
That is expected for this hardware and is upstream's own documented limit:
Stela starts a GPU through its NVIDIA driver, its hardware requirements name a
GeForce GTX 16 or RTX 20/30/40/50 series, and an Intel-only machine ends with
`STELA FATAL macOS cannot run without a GPU` (`Vendor::Intel => "Intel i915/xe
are not ported to Stela"`). macOS requires a Metal device, so there is no
software-rendering fallback to fall back to.

The Apple logo and progress bar seen during all of this are drawn by Stela
before the guest starts, which is why the installer could render while the
installed system cannot.

### What was learned about the Intel path

The pinned xe table does hold this laptop's GPU: `8086:7D51` sits in
`INTEL_ARL_H_IDS` in `include/drm/intel/pciids.h` and `xe_pci.c:514` binds that
family to `&mtl_desc`. So xe reports **METEORLAKE** for Arrow Lake H, with a
46-bit DMA mask, `is_dgfx` false (integrated), and the force-probe mark (xe
there is experimental upstream). That is now asserted by tests rather than
assumed:

- `crates/stela-driver-gpu/src/stack.rs`: `GpuStack::select` picks `INTEL` for
  `0x7d51`, driving xe, `(1 << 46) - 1` DMA limit, integrated.
- `crates/stela-driver-gpu/src/xe.rs`: `0x7d51` is in the 46-bit group, is named
  `METEORLAKE`, is not discrete, and is force-probed.
- `xtask/src/gpu/anv.rs` already listed `7d51` (as "Arrow Lake P"), so the ANV
  host lane covers this GPU without a change.

### The silent-hang fix

Because the step had no reportable boundary, the boot could not say where it
stopped. `Starting the GPU` now names its phase before entering it, on screen
and in the USB log (`output::flush_log` after each claim), through
`stela_boot_core::gpu_status::phase_entered`:

| line | when |
|---|---|
| `GPU START phase=claim` | before `claim_all` |
| `GPU START phase=linux claim_ms=N` | after the claim, before Linux starts |
| `GPU START phase=probe` | once Linux's first tasks are up, before the driver's probe |

The claim also flushes the USB log after each adapter, so the first stuck access
leaves a refusal line behind rather than nothing. `engine::step_watchdog`
carries the stage name while each phase runs (`the GPUs' claim`, `Linux's GPU
environment`), so a stop inside one names it.

### Verification (this session, on this machine)

| command | result |
|---|---|
| `cargo test -p stela-driver-gpu` | `DRIVER_GPU_EXIT=0` |
| `cd crates/stela-boot && cargo check --features xe --target x86_64-unknown-uefi` | `BOOT_XE_EXIT=0`, 3m57s |
| `cargo test -p stela-boot-core` | `CORE_TEST_EXIT=0` |

The second line is the first proof that the boot image can be built with the
Intel stack linked — `crates/stela-boot`'s `xe` feature pulls
`stela-drivers/xe-port`. The shipped image still requests only `nouveau-port`,
so this changes no default and nothing about how NVIDIA machines boot. Nothing
here was booted; the diagnostics and the xe-linked image are compiled, not run.

### How the work was done

Four `opencode` agents (`opencode/glm-5.3-flash-free`) ran in isolated git
worktrees on the roadmap's M3 items. Two produced usable diffs (the `0x7d51`
admission tests and the phase diagnostics) and those were reviewed and applied
by hand; the other two were still searching when the provider's free usage limit
ended the run (`Free usage limit reached`), so their worktree edits were
discarded, not merged. Worktrees were removed afterwards.

Not done, in the roadmap's order: GuC/HuC/GSC/DMC firmware selection for the
Meteor Lake descriptor, the xe probe itself (BAR, DMA mask, IRQ domain, PPGTT,
memory region), and the display path. Until the probe binds a DRM node, ANV has
nothing to open and `GpuStack::INTEL.vulkan` stays `None` — deliberately, since
no silent software fallback is allowed.

### The xe image is on the stick

Chasing the firmware step found it already done upstream: `gpu/before.rs`'s
`XE` part calls `recipes::require_xe`, and `recipes.rs`'s
`"Meteor Lake or Arrow Lake"` entry holds `0x7d51` with
`i915/mtl_guc_70.bin`, `i915/mtl_huc_gsc.bin`, `i915/mtl_gsc_1.bin` and
`i915/mtl_dmc.bin`, each pinned by digest, with a test already asserting it.
Only the blobs on the stick are missing. So the step that remains is the probe,
and the next fact has to come from the hardware.

The image was therefore built with Intel's driver in it:

    STELA_XE=1 cargo xtask boot build --release
    Finished `release` profile [optimized + debuginfo] target(s) in 3m 59s

| artifact | bytes | note |
|---|---|---|
| `stela.efi` | 204,379,648 | symbols kept, sha256 `08174ac6…f9c5` |
| `stela-deploy.efi` | 52,928,000 | stripped, what the stick boots |

Both carry `mtl_guc_70`, `mtl_huc_gsc`, `mtl_gsc_1`, `mtl_dmc`, and the new
`GPU START phase=` lines. The stick's `EFI/BOOT/BOOTX64.EFI` was replaced with
the deploy image (`eba04fb7…dd67`, matching the build byte for byte); the
NVIDIA-only loader that was there is kept at
`~/Downloads/stela-backup/BOOTX64-nvidia-2026-11-17.efi` (`c521aafc…bfe5`) to
put back without a rebuild.

What the next boot should show, if the pieces are as read: `Starting macOS`,
then `Starting the GPU`, then `GPU START phase=claim` and — because the stick's
firmware bundle predates the Intel stack and holds none of the four blobs —
`GPU …: the USB's firmware bundle lacks 4 of this chip's 4 files (…); `stela
repair` completes it`, refused, and the boot naming why no GPU started. That
refusal would be the first proof on this machine that Stela's Intel stack
selects, identifies and asks for the right firmware; it is not acceleration,
and it is not a working boot. **`stela repair` must not be run on this stick**:
it would rewrite the startup choices and bring the network medium back.

## Session 8: the first xe boot on the Arc 140T, and the BAR policy that stopped it (2026-11-17)

The xe image (`STELA_XE=1`, `stela-deploy.efi` `eba04fb7…dd67`) was booted on the
laptop. It reached the GPU step and printed the phase the diagnostics were built
for, then refused — for a reason that has nothing to do with Intel support. Stela
rejected the register BAR.

### What the stick's log says

`stela log /dev/sda -o /tmp/stick-boot5.txt` (6 boots; the newest ran from
t=10.3 s to `STELA STOPPED` at t=1170.9 s):

| line | meaning |
|---|---|
| `HOST GPU 0000:00:02.0 8086:7d51: xe will bind it` (t=18.7 s, before EBS) | the Intel stack selects this Arc 140T |
| `HOST FIRMWARE BUNDLE USE i915/mtl_guc_70.bin bytes=320320 for pci 8086:7d51`, and the same for `mtl_huc_gsc` (561,152), `mtl_gsc_1` (1,142,784), `mtl_dmc` (52,712) | **the stick's bundle already held all four Meteor Lake blobs** — no `stela repair`, no network, nothing to fetch |
| `HOST FIRMWARE phase=before-ebs images=25 bytes=6767957` | the bundle prefetched 25 images for this boot |
| `STEP Starting the GPU t=1125.710`, then `GPU START phase=claim` | the stage was entered and its phase printed |
| `GPU 0000:00:02.0 register BAR refused: Prefetchable BAR0=Some(Memory { address: 550024249344, size: 16777216, prefetchable: true, wide: true })` | BAR0 = `0x8010000000`, 16 MiB, 64-bit, prefetchable |
| `GPU 0000:00:02.0 refused: Refused("the GPU's register BAR is not an assigned register window")`, `STEP w … ms=8` | the claim ended in **8 ms** — it never hung |
| `STELA FATAL macOS cannot run without a GPU` … `STELA STOPPED code=0x11; power off to continue` (t=1170.932) | the boot stopped on its own |

Two corrections to the previous session's expectations. The four blobs were
already on the stick. And the long wait was **not** the GPU step: it was
`Taking control of the other processors`, which took **1056 s** on this boot
(36 s, 108 s and 248 s on earlier ones). Stela's own x86 GPU stage is
milliseconds; the picture that sat on "Starting the GPU" was the screen not
repainting after the stop, not a hang in the claim.

### Why the BAR was refused, and why the refusal was wrong

`stela-driver-gpu`'s `cold::register_bar` refused every prefetchable BAR:

    Some(Bar::Memory { prefetchable: true, .. }) => Err(RegisterBar::Prefetchable),

That rule is nouveau's. NVIDIA's BAR0 register window never decodes
prefetchably, so a prefetchable BAR0 on an NVIDIA function is some other window.
Linux on this same laptop, with `i915` bound to the device right now, reports

    0000:00:02.0 Region 0: Memory at 8010000000 (64-bit, prefetchable) [size=16M]
                 Region 2: Memory at 8000000000 (64-bit, prefetchable) [size=256M]

— the same address, size and flags Stela decoded. Intel's GTTMMADR really is
prefetchable on this platform, and its driver maps it as it finds it. Stela's
BAR reader was right; the policy around it was NVIDIA's.

### The fix

- `stack::Kernel` gains `prefetchable_registers: bool`: `false` for nouveau and
  amdgpu, `true` for xe, each with its evidence in the field's doc comment.
- `cold::register_bar` takes that flag. Every other refusal (absent, I/O,
  smaller than a page, unassigned) is unchanged for every vendor, so an Intel
  BAR that is small or unassigned is still refused.
- The screen no longer calls an Intel GPU an NVIDIA one. `gpu_status::refused`
  takes the vendor: an NVIDIA adapter is still named with the generation Stela
  needs, while another vendor is named as itself and quotes the claim's own
  words. The previous line read "the NVIDIA GPU at 0000:00:02.0 (device 7d51) is
  not supported", which is both false and useless in a report.

| check | result |
|---|---|
| `cargo test -p stela-driver-gpu` | `GPU_TEST_EXIT=0` (39 unit tests and the integration suites) |
| `cargo test -p stela-boot-core` | `CORE_TEST_EXIT=0` |
| `cd crates/stela-boot && cargo check --features xe --target x86_64-unknown-uefi` | `BOOT_XE_EXIT=0` |

The tests cover both policies: a prefetchable BAR is still refused for nouveau,
still `Small` or `Unassigned` under xe's policy, and an Intel refusal line
contains no "NVIDIA".

### Where the port stands

The claim now gets past the register BAR **on paper only**; it has not been run
again. The refusals behind it in the same function are the next ones: each
memory BAR against the host's aperture (`services.bar_resource` and the
physical-span check), the identity read of the first page, the cold reset and
bind, and then xe's own probe. BAR0 sits at `0x8010000000`, above 4 GiB, so the
aperture and mapping path is the most likely next wall. None of this is
acceleration, and nothing here ran under macOS.

The corrected image is on the stick and has **not** been booted. It was built
after the formatting and the checks above
(`STELA_XE=1 cargo xtask boot build --release`, 7m 36s, `BUILD_EXIT=0`) and
written to `EFI/BOOT/BOOTX64.EFI` as sha256 `10849f43…a44a`; the xe image that
produced this log is kept at
`~/Downloads/stela-backup/BOOTX64-xe-barfix-2026-11-17.efi` (`eba04fb7…dd67`),
and the NVIDIA-only loader before it at
`~/Downloads/stela-backup/BOOTX64-nvidia-2026-11-17.efi` (`c521aafc…bfe5`).

### Where the 17 minutes go, and the first cut at it

The log timestamps this step already, so no rebuild was needed to find the
cost. The newest boot's breakdown:

| segment | time | note |
|---|---|---|
| `the firmware ends its boot services` | 83.6 s | ExitBootServices itself took **131 ms** (`FIRMWARE boot services ended at try 1 of 4 in 131 ms`); the rest is the pre-exit work |
| `the firmware has left; its USB controllers` | 62.2 s | two xHCI controllers handed away from the firmware |
| `the local APIC` | 10.4 s | x2APIC mode chosen |
| `stopping the firmware's processors` | **774.3 s** | 15 INIT deliveries, `apic=0x8 … 0x42`, spaced ~48 s apart, every one `destination_ack=unobserved` |

`Apic::init_only` cannot be the 48 s: this processor is x2APIC (so
`wait_delivery` returns at once, the delivery-status bit having been removed)
and Intel family 6 (so `settles_at_once` skips the 10 ms INIT wait), leaving a
`send` of two register writes and no delay. What sat between the deliveries was
therefore the `NATIVE FIRMWARE INIT delivered apic=…` line each one printed.

`quiesce_firmware_aps` no longer prints per delivery; the step prints its count
and total once, with `ms=`. If the 774 s collapses on the next boot, the cost
was the post-ExitBootServices console path and every later test gets ~13
minutes back. If it does not, the cost is in the INIT itself and the next log
says so by name. Either way the answer costs one boot.

## Session 9: the boot tax is gone, the claim passed, and Linux started (2026-11-17)

One boot answered three questions at once, and the newest log
(`/tmp/stick-boot6.txt`, 7 boots kept) is the first run on this machine where
Linux started at all.

### The 17 minutes were the log lines

| | before | after |
|---|---|---|
| `Taking control of the other processors` | 1056 s | **36 s** |
| `NATIVE FIRMWARE INIT deliveries=15` | 774 s, ~48 s per delivery | **`ms=0`** |
| `STEP Starting the GPU` at | t=1125.7 s | **t=68.4 s** |

`NATIVE FIRMWARE INIT deliveries=15; … ms=0` settles it: the deliveries were
always instantaneous, and the 774 s was the one `output::arguments` line printed
after each — a post-ExitBootServices console write costing ~48 s. Removing that
line (the caller reports count and time once) cut the step by a factor of 29
and the boot to the GPU by 16×. This is the cheapest change in the port so far
by wall-clock returned.

### The claim got past the register BAR

    GPU START phase=claim
    GPU 0000:00:02.0: reset by Function Level Reset; kept BAR0 16 MiB, BAR2 256 MiB
    GPU START phase=linux claim_ms=234

So the per-vendor `prefetchable_registers` fix (session 8) did what it was meant
to: the register BAR was admitted, every memory BAR was mapped against the host's
aperture, the function was reset by FLR, and the cold bind completed — all in
234 ms. **This is the first successful Intel GPU claim on this hardware.**

### Linux started, then the record stopped

Seven lines, all at t=68.6 s, then nothing:

    LINUX <6> pid_max: default: 32768 minimum: 301
    …
    LINUX <6> VFS: Finished mounting rootfs on nullfs
    LINUX <6> workingset: timestamp_bits=62 (anon: 57) max_order=21 bucket_order=0 (anon: 0)

No vmlinux panic, no watchdog stop, no fatal: the boot is inside Linux's own
bring-up with the xe stack linked. The screen is black because the firmware
console ends with ExitBootServices and nothing draws again until Stela's own
display path runs — earlier runs never got this far, which is why the step list
was always visible.

The record stops here for a mundane reason: `output::flush_due()` is called by
the engine's service loop, and no service loop runs beside the GPU start, so
Linux's lines stayed in memory. `GpuViews::report` now calls it per line (it
starts due stores and never waits), so a start that never returns leaves its
last lines on the stick.

What this does **not** mean: Linux has not finished starting, xe has not
probed, no firmware has been loaded on the GPU, nothing has drawn anything, and
macOS has not run. The GPU still does not serve a Metal device.

### Session 9, continued: why the silence, and the way out of it

Two boots (8 total in `/tmp/stick-boot7.txt`) now end at the same place, and the
second one already carried the per-line store that was meant to bring more
lines:

    [398.704] GPU START phase=linux claim_ms=234
    [398.735] LINUX <6> pid_max: default 32768 minimum 301
    …
    [398.743] LINUX <6> workingset: timestamp_bits=62 (anon: 57) max_order=21 bucket_order=0 (anon: 0)

Two things were established by reading the code rather than by booting.

**1. `flush_due()` cannot store during this phase, so that earlier change was
dead on arrival.** `output::flush_where` returns at its first check when
`engine::inside_host_drivers()` is set, and the whole GPU start runs inside the
host drivers. The image built for that hypothesis could never have written
anything. `GpuViews::report` now calls `output::store_usb_log()` at most once a
second instead — the start's own store, which waits for the device.

**2. The silence is structural, not bad luck.** `workingset_init()` is not part
of `start_kernel` here: Stela's compatibility layer calls the kernel's init
functions itself, in `crates/stela-linux/src/compat/kernel/task_bootstrap.c`,
ending with

    error = workingset_init();
    if (!error) error = fcntl_init();
    resource_phase = COMPLETE;

and then creates PID 1 and kthreadd with `kernel_thread`, after which PID 1
passes two completions that had **no timeout**:

    wait_for_completion(&kthreadd_published);
    wait_for_completion(&kthreadd_entered);

The loop that would report a stall (`wait_for_child`, which prints every
`WAIT_REPORT_SECONDS = 2`) can only speak between dispatch steps, and these
waits happen *inside* one step. So a kthreadd that never runs, or a completion
that never arrives, produces exactly what the stick holds: complete silence,
forever, with no watchdog line, because the step never returns.

Note also what the last line actually proves. It is printed from inside
`workingset_init()` (mm/workingset.c) *before* the function returns, so nothing
in the record yet says whether that call came back.

### What the next boot is for

The new image names each boundary out loud and bounds each wait:

    stela: built-in initcalls: workingset done (%d)
    stela: built-in initcalls done; resource roots complete
    stela: threads: supervisor claimed pid %d
    stela: threads: kthreadd claimed pid %d
    stela-kthreadd: running, waiting for its publication
    stela-kthreadd: entering its loop
    stela-init: published; waiting for kthreadd
    stela-init: kthreadd published; waiting for it to enter
    stela-init: kthreadd entered; starting the workqueues

`kthreadd_published` now times out after 10 s and `kthreadd_entered` after 30 s,
each into a `panic()` that Stela records, instead of waiting for ever. Whichever
of those lines is last in the next log names the stall:
- nothing after `workingset done` → the hang is inside `workingset_init` itself;
- `threads: kthreadd claimed` but no `stela-kthreadd: running` → kthreadd was
  created and never dispatched;
- `stela-init: kthreadd published; waiting for it to enter` alone → PID 1 runs
  and kthreadd does not;
- a panic line → the timeout fired and named which completion never came.

### The black screen, explained (hypothesis, testable)

The screen goes black on exactly the boots whose claim succeeded — that is, the
ones where `reset by Function Level Reset` ran. The framebuffer Stela draws the
step list into lives in the Intel GPU's aperture, so an FLR of that function
takes the display with it, and nothing re-initializes it before macOS would.
Earlier boots refused the register BAR, never reset the GPU, and kept their text
on screen. That is consistent with every observation so far, and it is not a
crash: the machine was still running the boot after the screen went dark.

### Tooling: the stick needs no privilege at all

`udisksctl mount -b /dev/sda1` mounts the ESP at
`/run/media/<user>/EFI` as the logged-in user, and that mount is writable. Both
jobs that used to need elevation now do not:

    udisksctl mount -b /dev/sda1
    cp -f target/boot/x86_64-unknown-uefi/release/stela-deploy.efi \
        /run/media/$USER/EFI/EFI/BOOT/BOOTX64.EFI
    sync && sha256sum /run/media/$USER/EFI/EFI/BOOT/BOOTX64.EFI
    udisksctl unmount -b /dev/sda1

The boot log is `EFI/stela/boot.log` on that same partition, so reading it needs
no raw device access either. Passwordless privilege is therefore not needed for
this port's test cycle at all.

## Session 10 — the boot reaches the installer, and the install completes (2026-11-17)

Image `99f8e254…` on the stick, firmware boot counter 9, `HOST version=0.4.2`,
Core Ultra 7 255H. This is the boot that answers session 9's open question, and
it answers it by going much further than "the stall is named".

**The stall is gone.** Boots 7 and 8 ended inside `workingset_init()`; boot 9
prints straight through the boundaries the diagnostic image names:

    [ 399.521] LINUX <6> workingset: timestamp_bits=62 (anon: 57) max_order=21 bucket_order=0 (anon: 0)
    [ 399.521] LINUX <6> stela: built-in initcalls: workingset done (0)
    [ 399.521] LINUX <6> stela: built-in initcalls done; resource roots complete
    [ 399.522] LINUX <6> stela: threads: supervisor claimed pid 1
    [ 399.522] LINUX <6> stela: threads: kthreadd claimed pid 2
    [ 399.522] LINUX <6> stela-init: published; waiting for kthreadd
    [ 399.522] LINUX <6> stela-init: kthreadd published; waiting for it to enter
    [ 399.522] LINUX <6> stela-kthreadd: running, waiting for its publication
    [ 399.522] LINUX <6> stela-kthreadd: entering its loop
    [ 399.522] LINUX <6> stela-init: kthreadd entered; starting the workqueues

No panic and no step-watchdog stop fired, so the stall was never a missing
timeout: the two completions PID 1 passes were reached and passed. The bound the
image added (10 s, 30 s) did not have to fire.

**The guest then ran macOS for 22 minutes.** The engine's run label is
`Starting the macOS installer` (`engine.rs`, set from `restore`), and at the end
of the run Stela reported

    [2451.973] STEP o Starting the macOS installer (1310 s) ms=1310396

so the macOS restore/install environment executed from t=1141 to the stop. Its
own numbers: `GUEST PERFORMANCE retired=1.56e12`, about 0.9–1.0 billion guest
instructions per second, `STATISTICS blocks: 14101651663` with 99% chained.

**The install completed.** `restore.rs` prints its ledger as the installer's
checkpoint protocol advances, and every checkpoint the log shows succeeded:

    seal_system_volume            COMPLETE=true
    create_system_snapshot        COMPLETE=true
    set_autoboot_delay            COMPLETE=true
    unmount_filesystems           COMPLETE=true
    cleanup_boot_command          COMPLETE=true
    cleanup_recovery_os_volume    COMPLETE=true
    cleanup_check_result          COMPLETE=true
    cleanup_run_leaks             COMPLETE=true
    cleanup_send_crash_logs       COMPLETE=true
    cleanup_send_final_status     COMPLETE=true
    AMRError                      0

then, from `restore.rs:1072`, the terminal line of `Mode::DevelopmentInstall`:

    DEVELOPMENT INSTALL protocol complete; TSS enabled, FDR offline/unprovisioned; installed boot remains unqualified

The target partition is the internal WD PC SN5000S, `nvme0n1p4`, 87,787,831,296
bytes, partition label `Stela macOS`; Stela's own disk counters for that boot
(`HOST DISK IO serial=25491H809160`) read 28,019,160 KiB written and 48,142,122
KiB read. The installer then asked the machine to restart — `STOP reset:
Restart: smc: MBSE restart command` — and Stela reported

    [2452.026] POWER the firmware starts its entry Boot0005 again after the restart
    [2452.026] STELA RESTART code=0x15

**What it looked like: nothing.** `CURSOR moved_per_wall_second=0.0`, and the
guest console Stela captures is blank (`GUEST|` rows are VGA attribute bytes,
0x07, with no text). The install ran headless, which is consistent with the
hypothesis at the end of session 9: the claim's Function Level Reset takes the
firmware framebuffer down and nothing re-initializes it.

**What this boot still does not say: whether xe probed.** Stela holds the host
drivers' lock for Linux's whole start and refuses to store the USB's log from
inside a driver call — the boot log says so in as many words, at t=399.478:

    BOOT LOG not stored: this processor stopped inside the host drivers

Everything printed between that line and the engine's run is on the screen only,
so `GPU START claim_ms=… linux_ms=… probe_ms=…` and `STEP o Starting the GPU`
are not in the file. All that the timing says is that the engine ran for
1310 s (t=1141 to the stop), which bounds the start plus everything the engine
does before it runs the guest at about 742 s. It does **not** say the start
succeeded: see "Session 10, continued" below, which corrects the first reading
of this boot — `open_gpus` returning false only skips `gpu::serve`, and the
`macOS cannot run without a GPU` line is a pre-start judgement of the
configured stack, so a 22-minute headless install is exactly what a failed
start looks like. The change this audit made is the one that answers it: the
start's report and step ending are now printed inside the start's own scope and
stored from there.

**Next boot decides the rest.** The install is sealed and the machine was asked
to restart into it; Boot0005 is Stela again, so the next boot should build a boot
bundle from the installed system (`boot_policy`, `bundle::from_disk`) instead of
serving the restore. What that boot reports — a first-boot macOS, a new `STELA
FATAL`, or a stop — is the next measurement.

## Session 10, continued — boot 9's log audited line by line (2026-11-17/18)

The ring on the stick holds boots 5 to 9. Boot 9 is the one that installed
macOS; the four before it are the earlier attempts (boot 5 stopped placing the
board's devices, boot 6 refused the register BAR, boots 7 and 8 stopped inside
`workingset_init()`). Everything below is boot 9 unless said otherwise.

### Where the 41 minutes went

    0 –   27 s  loader, `stela.cfg`, GPU found, translation cache (211 MB, 8 s)
   27 –  360 s  Taking control of the other processors          330.9 s
  360 –  399 s  system engine, host processor state, ACPI, devices, disks/USB
  399.2– 399.5 s  GPU claim                                          234 ms
  399.5– 1141 s  the GPU's start, of which Linux printed 12 lines and stopped 742 s
 1141 – 2452 s  the macOS restore environment                   1310 s

The 331 s step breaks down as its own detail lines say:

    83.7 s  from "the firmware ends its boot services" to the next part
            (the exit itself was "try 1 of 4 in 132 ms" at t=84.0, so ~57 s
            pass before the exit is even called, and ~33 s after it)
    62.3 s  `usb_legacy.take`: two xHCI controllers, each reported
            "not owned by the firmware" — i.e. no wait was needed, so ~30 s
            per controller is spent inside the capability's own MMIO access,
            which is firmware/SMM territory (issue 36), not a Stela timeout
    10.4 s  choosing x2APIC and reading the firmware's DMAR
    49.5 s  quiescing 15 firmware processors (INIT deliveries, no ack)

This audit adds three `status::progress` calls inside the 83.7 s part (the
map buffer, the exit's call, the fault reporter) so the next boot names which
of them owns the ~90 s; that step prints only on the screen otherwise.

### The GPU's start: no display, and probably no success

Four lines that a successful start would have printed are absent:

    GPU START claim_ms=… linux_ms=… probe_ms=… total_ms=…
    [ok] Starting the GPU
    GPU <name>: display runs <w> x <h> at … mHz
    DISPLAY no GPU display yet: looking for one in 1920x1200 once a second

`gpu::serve` runs only when `open_gpus` returned true (`start.rs`), and every
one of its paths prints. Their absence is consistent with a start that failed
after the claim — but it is not proof, because the same host drivers' lock that
swallowed the step lines would have swallowed these too. What is certain is
that nothing was presented all boot: `CURSOR moved_per_wall_second=0.0` in
every report, and `samples=0`.

The dead screen is not a separate mystery: `display.rs` states that binding a
GPU ends the firmware's framebuffer on it, and that the console then moves to
the output's front buffer through the driver's KMS. With no display taken
there is no front buffer to move to, so the panel stays dark *and* the console
Stela prints to is gone. That is why a 22-minute install ran with nothing on
screen and why the guest never drew a frame.

**Correction to this session's earlier note, and to the roadmap and
`KNOWN_LIMITATIONS`:** a failed GPU start does *not* end the boot. The stop
message `macOS cannot run without a GPU` comes from `gpu::no_metal_device()`
(`start.rs:157`), which runs *before* the start and judges the configured
stack, not the live device; `open_gpus` returning false only skips
`gpu::serve` and the boot carries on. Boot 6 shows the pre-check firing for a
refused claim, not the start failing. So boot 9's headless install is exactly
what a failed start looks like, and the pre-check cannot catch it.

### What the guest said while it installed

Stela captures the emulated console, and the ring holds its last 113 s
(t=2334.8–2448.2). It is the install's own record and it agrees with the
ledger:

    [15:35:35.0710-GMT]{1>5} CHECKPOINT FINAL-MONITOR:(SUCCESS)  ... reboot
    restore-step-user-progress = 100
    CHECKPOINT END: RESTORED:[0x068B] seal_system_volume
    CHECKPOINT END: RESTORED:[0x0669] create_system_snapshot
    CHECKPOINT END: RESTORED:[0x0640] set_autoboot_delay
    CHECKPOINT END: RESTORED:[0x0642] unmount_filesystems
    CHECKPOINT BEGIN/END: [0x0651] cleanup_failure_warning
    apfs_log_op_with_proc: disk1s1 mount-complete volume Macintosh HD

Failures worth fixing, in the guest's words:

- `Couldn't retrieve SMC Key AC-R. Got SMC error: -3` followed by
  `unable to kick port micro - Couldn't set AC-R to 0`. Stela's SMC model has
  no `AC-R` key. A key to add.
- `virtual IOReturn IONVMeController::SendShutdownNotification(bool)::2161:
  CC=0x464001 timeout=25000`. The guest asked the controller to announce its
  shutdown and the model did not answer in the guest's 25 s. Every restart
  therefore ends with an NVMe controller that the guest believes is stuck;
  the install still sealed, but this is the one place where a real data
  flush could be skipped.
- `libpartition: unexpected partition 'iSCPreboot' - skipping`: a stale
  `iSCPreboot` left by an earlier attempt. The installer skips it, so the
  layout now carries a partition nothing uses.
- `getVolumeGroupMountFrom:9463: … failed with error 2` (twice),
  `nx_volume_group_update:706: … Volume Update role 0xc0 is not a System or
  Data volume`, `Tried to unmount a volume at '/mnt5' that wasn't mounted`,
  `sock 9: shutdown(SHUT_RDWR) failed: Socket is not connected`,
  `pre-reboot: non-boot task posix_spawn() failed: 2`, and launchd's
  `failed lookup` for `logd`, `coreservicesd`, `cfprefsd` and
  `notification_center` — all benign on a restore ramdisk.
- `mDNS [ ]: no usable interfaces found` (seven times): the guest has no
  network interface at all, which is the Wi-Fi failure below seen from inside.

### Host devices that did not start

Each is a missing feature rather than a crash, and each is named by Stela:

    HOST DEVICE FAILED pci=0000:00:14.3 (Wi-Fi)  bus mastering stays off until
        the device is taken over after Stela wrote the register 0x82
    HOST DEVICE FAILED pci=0000:00:0d.2 (USB4)   USB4 host: NHI cold ownership:
        ResetRequired
    PLATFORM FAILED \_SB_.PC00.I2C1.ETPD         native mapping: OutsideResource
    PLATFORM FAILED PNP0CA0                      UCSI: HardwareInactive
    USB UNUSED port 2 interface 0 (01/01/00)     unsupported or malformed UAC2
                                                 function descriptors
    USB UNUSED port 9 interfaces 0-4 (0e/…)      no class driver for this
                                                 interface (a camera)
    HOST IOMMU VT-d unit 0xfc800000/0xfc801000   UnsupportedStructure(5)
    HOST IOMMU every unit left untranslated and off

So: no Wi-Fi, no USB4, no trackpad, no USB-C connector management, no camera,
no USB audio, and DMA that the IOMMU does not confine. Seven IOAPIC GSIs are
left to the firmware as SMI pins, and `no IOMMU: device DMA unrestricted`
appears once the units are dropped.

### Log plumbing: two faults, both fixed tonight

1. **Step lines were printed where the store is refused.** `status::begin`,
   `done`, `warn` and `fail` all call `output::flush_log()`, and inside the
   host drivers' lock that call refuses and says so once
   (`BOOT LOG not stored: this processor stopped inside the host drivers`,
   t=399.478). The GPU start holds that lock for its whole length, so its
   step lines and its report reached the screen and never the stick. `status`
   now asks for the start's own store first (`status::store_logs`), and the
   start's report is printed inside the start's scope with a store behind it.
2. **The console floods the ring.** The last 60 s of boot 9 produced 2.2 MB of
   `GUEST-FRAGMENT`/`GUEST|` rows — mostly blank cells as `\x07` attribute
   bytes — in a 4 MiB ring. A long boot can therefore lose its own beginning.
   Not changed tonight; the capture should drop blank cells and repeat rows.

Also noted, minor: `HEAP … cores=-43400M/…` prints a negative share for one
core. The figure is a per-core allocation-versus-free counter, so a block
allocated on one core and freed on another drives it negative; the size is
larger than the whole heap, which suggests a wrap rather than a real number.
Diagnostics only.
