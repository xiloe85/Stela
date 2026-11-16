#[cfg(feature = "amdgpu-port")]
#[path = "build/amdgpu.rs"]
mod amdgpu;
#[cfg(feature = "linux-drm")]
#[path = "build/image.rs"]
mod image;
#[cfg(feature = "linux-drm")]
#[path = "build/imports.rs"]
mod imports;
#[cfg(any(feature = "nouveau-port", feature = "xe-port"))]
#[path = "build/kbuild.rs"]
mod kbuild;
#[cfg(feature = "linux-kernel")]
#[path = "build/native.rs"]
mod native;
#[cfg(feature = "nouveau-port")]
#[path = "build/nouveau.rs"]
mod nouveau;
#[path = "build/nouveau_firmware.rs"]
mod nouveau_firmware;
#[cfg(feature = "xe-port")]
#[path = "build/xe.rs"]
mod xe;

/// The Vulkan drivers `cargo xtask gpu nvk` linked into the image's archives,
/// as it lists them beside them: with `metal-linked`, Intel's sets
/// `stela_vulkan_intel` and AMD's `stela_vulkan_amd`, so the crate names only
/// loader entries the image holds. NVK is always there.
fn vulkan_drivers() {
    const OPTIONAL: [(&str, &str); 2] =
        [("intel", "stela_vulkan_intel"), ("amd", "stela_vulkan_amd")];
    for (_, cfg) in OPTIONAL {
        println!("cargo:rustc-check-cfg=cfg({cfg})");
    }
    if std::env::var_os("CARGO_FEATURE_METAL_LINKED").is_none() {
        return;
    }
    let list = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/gpu/nvk/vulkan-drivers");
    println!("cargo:rerun-if-changed={}", list.display());
    let listed = std::fs::read_to_string(&list).unwrap_or_default();
    for (name, cfg) in OPTIONAL {
        if listed.lines().any(|linked| linked == name) {
            println!("cargo:rustc-cfg={cfg}");
        }
    }
}

fn main() {
    nouveau_firmware::validate();
    vulkan_drivers();
    // Active ports declare their native source inputs below. Without this
    // baseline Cargo watches every vendored file even when no C port is used.
    println!("cargo:rerun-if-changed=build.rs");
    #[cfg(feature = "amdgpu-port")]
    amdgpu::tables();
    #[cfg(feature = "xe-port")]
    xe::tables();
    // Clippy reads Rust and links nothing, so it needs no C objects.
    println!("cargo:rerun-if-env-changed=CLIPPY_ARGS");
    #[cfg(feature = "linux-kernel")]
    if std::env::var_os("CLIPPY_ARGS").is_some() {
        let tests = stela_c_build::linux_build::Tests::default();
        tests.write();
        return;
    }
    #[cfg(feature = "linux-kernel")]
    let (linux, mut tests) = (
        native::Linux::new(),
        stela_c_build::linux_build::Tests::default(),
    );
    #[cfg(feature = "linux-kernel")]
    linux.tests(&mut tests);
    #[cfg(feature = "linux-drm")]
    {
        let drivers = vec![
            #[cfg(feature = "nouveau-port")]
            nouveau::port(&linux),
            #[cfg(feature = "xe-port")]
            xe::port(&linux),
            #[cfg(feature = "amdgpu-port")]
            amdgpu::port(&linux),
        ];
        image::link(&linux, drivers);
    }
    #[cfg(feature = "linux-kernel")]
    tests.write();
}
