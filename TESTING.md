# TESTING

## 0. Rules

- A test counts only if it **ran**; this file records what was executed and
  what was not.
- Host tests (no GPU) and hardware tests are separated everywhere.
- Simulated tests (DRM shims) are labelled as such; they validate
  compilation, ioctl coverage and codegen reachability, **not** hardware
  behaviour.
- Nothing related to booting macOS touches the internal disk or the existing
  EFI partition. Boot media is a USB stick or an external SSD.

## 1. Test layers in Stela

| Layer | What it proves | Hardware |
|---|---|---|
| Rust unit tests (`cargo test -p stela-driver-gpu`, `-p stela-boot-core`, …) | admission logic, table parsing, capability gating, ABI lowering | none |
| C hosted tests (`crates/stela-driver-gpu/src/*/tests/*.c`, `crates/stela-mesa`) | AIR→NIR, NAK/ACO/BRW plumbing, command encoders | none |
| `cargo xtask gpu anv-test` | ANV built with the image's options creates a device, builds a BRW compute pipeline and submits one dispatch on a **no-op xe shim**; every xe ioctl ANV issues must be one Stela's xe registers | none (shim) |
| `cargo xtask gpu radv-test` | same for RADV/ACO on amdgpu | none (shim) |
| `cargo xtask check` | clippy/typecheck lanes for the host crates and the UEFI image | none |
| Real hardware | execution on Arc 140T | **required** |
| Boot log analysis | what Stela's Linux environment actually did with the GPU | **required** |

## 2. Intel-specific tests to add (this port)

1. **Admission**: `metal::intel::admit_device` for
   - `0x9a49` Tiger Lake (exists),
   - `0x56a0` DG2 (exists),
   - **`0x7d51` Arrow Lake-P / Arc 140T (this laptop — to add)**,
   - a device outside xe's table → `Refusal::UnknownDevice`.
2. **xe table**: `xe::admits(0x7d51)`, `xe::platform(0x7d51)` =
   integrated + display, `xe::dma_bits(0x7d51)`.
3. **ANV host test for ARL**: add `("7d51", "Arrow Lake P", false)` to
   `xtask/src/gpu/anv.rs`'s `PLATFORMS`.
4. **Real-GPU compute**: ANV built from the same Mesa options, linked
   against the host's libdrm (not Stela's compat layer), run on
   `/dev/dri/renderD128`, dispatch a kernel, compare with a CPU reference.
   This is the first test that proves Arc hardware execution at all.
5. **Memory stress**: repeated alloc/free of buffers and textures at the
   sizes the macOS compositor uses, on a 14 GiB-usable machine, with the
   allocation-failure diagnostics exercised deliberately.

## 3. Build and run

```sh
# sources (sparse submodules + Stela's patches); ~GBs, slow on this machine
cargo xtask gpu fetch

# host crate tests
cargo test -p stela-driver-gpu --features metal
cargo test -p stela-boot-core

# ANV on a no-op xe shim, per platform (needs host tools, see §4)
cargo xtask gpu anv-test --jobs 2

# the UEFI image (needs LLVM + Mesa built for the image; hours, RAM-hungry)
cargo xtask gpu nvk --jobs 2
cargo xtask dist
```

Host requirements for the ANV host test (from `xtask/src/gpu/clc.rs`):
LLVM + Clang development files (host has **23.1.3** ✓), SPIRV-Tools,
**SPIRV-LLVM-Translator**, glslang ≥ 12.2 (host has 16.6.0 ✓), meson
(1.12.1 ✓), ninja (✓), pkg-config with libdrm (2.4.134 ✓).

## 4. Status of checks on this machine (2026-11-16)

| Check | Result |
|---|---|
| Hardware inventory (`lscpu`, `lspci -nnk`, `glxinfo -B`, `lsusb`, `aplay -l`, DRM sysfs) | **ran**, results in HARDWARE_COMPATIBILITY.md |
| `cargo xtask gpu fetch` | **ran, succeeded** — all five submodules at their pins, patches applied (`third_party/*` on branch `stela`) |
| `cargo check -p xtask` | **ran, passed** (1m23s) after adding `7d51` to `xtask/src/gpu/anv.rs` |
| `cargo test -p stela-driver-gpu --features metal` | **ran, passed** — 639 tests, 0 failed, 1 ignored (+ 3 kbuild, 1 llvm-passes) |
| `cargo test -p stela-driver-gpu --features xe-port,metal -- metal::intel` | **ran, passed** — 4 Intel admission tests, after two build fixes (`kbuild` gate, xe's import list). This compiles the Intel composite: xe + shared i915 display + DRM core, 352 C objects |
| `cargo test -p stela-driver-gpu --features xe-port,metal` (all) | **ran, passed** — 654 tests, 0 failed, 1 ignored |
| `vulkaninfo --summary` | **ran, passed** — Vulkan **1.4.354**, `Intel open-source Mesa driver` (ANV), `Intel(R) Graphics (ARL)`, `INTEGRATED_GPU`, Mesa 26.2.5 (saved to `vulkan-arl-140t.txt`) |
| `cargo xtask gpu anv-test` | **ran, passed** after installing `spirv-llvm-translator`: ANV built with the image's options ran on xe's ioctls for **7 platforms including `0x7d51`**: `Intel(R) Graphics (ARL) (0x7d51, integrated), NIR options for 6 stages, compute pipeline of 12 instructions in 208 bytes, one dispatch submitted`. **No GPU behind it** — the shim answers the ioctls; this validates device creation, BRW codegen and Stela's xe submission interface, not silicon execution |
| `cargo xtask gpu nvk` / `dist` (image build) | **not run** — needs LLVM for UEFI + Mesa for the image; hours of compile, ~1 GB RAM free on this machine |
| Real-GPU dispatch on Arc 140T | **not run** |
| Stela boot on this laptop | **not run** |

Missing host packages for the Intel test lane (install only if you accept an
Arch package transaction on this machine):

```sh
# names as packaged on Arch
spirv-llvm-translator   # provides llvm-spirv + SPIRV-LLVM-Translator.pc
vulkan-tools            # vulkaninfo, to record this GPU's Vulkan capabilities
```

No test in this repository has been reported as passing that was not
executed.

## 5. Boot-log analysis procedure

1. Write the USB/external disk from another machine (`sudo ./stela`), or
   build `stela.efi` and place it on an ESP.
2. Boot the laptop from it; do not modify the internal disk.
3. Recover the log on the machine that made the stick:
   `sudo ./stela log --output log.txt`.
4. Look for, in order: PCI function presented → `Vendor::Intel` selection →
   `xe` binding → BAR0 → DMA mask → firmware (GuC/HuC/GSC/DMC) load →
   GT init → DRM render node → ANV device creation → Metal device.
5. One variable per boot. Never change several boot parameters at once.
