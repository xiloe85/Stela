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
| `cargo xtask gpu fetch` | **started**, did not complete in this session (see DEVELOPMENT_LOG) |
| `cargo xtask gpu anv-test` | **not run** — blocked on submodule fetch and possibly `llvm-spirv` |
| `cargo test -p stela-driver-gpu` | **not run** (needs the fetch for the generated xe tables) |
| `cargo xtask dist` / image build | **not run** — hours of compile, tight RAM |
| Real-GPU dispatch on Arc 140T | **not run** |
| Stela boot on this laptop | **not run** |

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
