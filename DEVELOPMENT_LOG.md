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
