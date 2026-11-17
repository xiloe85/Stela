//! One vendor's GPU driver stack as data: the Linux kernel driver that binds
//! the PCI function, the Mesa Vulkan driver over it, and the few facts and
//! driver-internal calls Vulkan does not carry. Everything above and below
//! (the Linux environment, the DRM ioctl path, the Metal device, the AIR
//! compiler, the gate) is shared and reads a vendor only through this.
//!
//! [`GpuStack::select`] chooses by PCI vendor. Whether a device of that
//! vendor is driven is then the kernel driver's own ID table's answer, when
//! Linux matches the function against the registered driver.
use crate::{Adapter, Vendor};
use core::ffi::c_void;

/// A device-control call on the bound GPU's open DRM file: the request code
/// and its argument, answering Linux's result (zero or a negative errno).
pub type Control<'a> = &'a mut dyn FnMut(u32, *mut c_void) -> i32;

/// The Linux kernel driver of a stack.
pub struct Kernel {
    /// The name its `pci_driver` registers under.
    pub driver: &'static str,
    /// Whether the driver's ID table holds the adapter, or why it does not.
    /// One it does not hold is left as firmware set it.
    pub admits: fn(&Adapter) -> Result<(), &'static str>,
    /// The BAR of the device's register window, whose first page the boot
    /// reads before the function's reset.
    pub registers: usize,
    /// Whether that window decodes prefetchably, so a prefetchable BAR is
    /// the window rather than another one (`cold::register_bar`).
    pub prefetchable_registers: bool,
    /// The address bits of the DMA mask the driver sets on this device; a
    /// mapping past it is refused, not truncated.
    pub dma_bits: fn(&Adapter) -> u8,
    /// Whether the driver's own table describes the device as built into
    /// the processor, sharing its memory and on its fabric rather than a
    /// PCIe link. [`metal_devices`] reads it.
    pub integrated: fn(&Adapter) -> bool,
    /// Whether the bound device is built into the processor, by the driver's
    /// own query, for a driver that decides it during the probe (amdgpu's
    /// IP discovery). Where there is no such query, or it does not answer,
    /// [`Self::integrated`] stands. [`metal_devices`] reads the bound GPU's
    /// answer.
    pub probed_integrated: Option<fn(Control) -> Option<bool>>,
    /// The driver's own identifier of the bound chip, by its query ioctl.
    pub chip: fn(Control) -> Option<u32>,
    /// The bytes of device memory the driver makes available.
    pub memory_bytes: fn(Control) -> Option<u64>,
}

/// One vendor's stack.
pub struct GpuStack {
    pub vendor: Vendor,
    pub kernel: Kernel,
    /// Admission of the stack's Vulkan device as a Metal device. A stack
    /// without one drives displays only.
    #[cfg(feature = "metal")]
    pub metal: Option<crate::metal::open::Admission>,
    /// The Vulkan driver linked into the image. A stack without one drives
    /// displays only. This crate's own test binary links no driver archive,
    /// so its descriptors hold none.
    #[cfg(all(feature = "metal-linked", not(test)))]
    pub vulkan: Option<crate::metal::native::Driver>,
}

impl Kernel {
    /// The widest DMA address the driver's mask allows `adapter`.
    pub fn dma_limit(&self, adapter: &Adapter) -> u64 {
        u64::MAX >> (64 - (self.dma_bits)(adapter))
    }
}

/// Every stack this build holds.
pub static STACKS: &[&GpuStack] = &[
    &NVIDIA,
    #[cfg(feature = "amdgpu-port")]
    &AMD,
    #[cfg(feature = "xe-port")]
    &INTEL,
];

/// nouveau with GSP firmware, NVK and NAK.
pub static NVIDIA: GpuStack = GpuStack {
    vendor: Vendor::Nvidia,
    kernel: Kernel {
        driver: "nouveau",
        // nouveau's table holds every NVIDIA display function; what it does
        // with a chip it does not know is its probe's answer.
        admits: |_| Ok(()),
        registers: 0,
        // BAR 0 decodes non-prefetchably on every NVIDIA display function;
        // a prefetchable one there is not the register window.
        prefetchable_registers: false,
        // The Turing-and-later MMUs' `dma_bits` (`nvkm/subdev/mmu/tu102.c`
        // 47, `gh100.c` 52), which `nvkm_device_pci_new` sets as the mask.
        dma_bits: |_| 47,
        // nouveau drives discrete GPUs only (`nvkm_device_pci_new`); Tegra's
        // built-in GPUs are platform devices, not PCI functions.
        integrated: |_| false,
        probed_integrated: None,
        chip: crate::nouveau_chip::query::chipset,
        memory_bytes: crate::nouveau_chip::query::memory_bytes,
    },
    #[cfg(feature = "metal")]
    metal: Some(crate::metal::nvidia::ADMISSION),
    #[cfg(all(feature = "metal-linked", not(test)))]
    vulkan: Some(crate::metal::nvidia::DRIVER),
};

/// amdgpu with its display core, and RADV's admission. Its Vulkan driver is
/// not given yet, even in an image that holds RADV: a PC with an AMD and
/// another GPU would then serve a Metal device on each, which no rule
/// chooses between yet, and RADV has run on no AMD GPU under Stela. Until
/// then its GPU shows the frames another GPU draws, on the displays wired to
/// it.
#[cfg(feature = "amdgpu-port")]
pub static AMD: GpuStack = GpuStack {
    vendor: Vendor::Amd,
    kernel: Kernel {
        driver: "amdgpu",
        admits: crate::amdgpu::admits,
        // `rmmio_base` of every family from Bonaire on (`amdgpu_device.c`);
        // BAR 0 is the memory aperture. The older families are refused.
        registers: 5,
        // BAR 5 decodes non-prefetchably, as firmware programs it.
        prefetchable_registers: false,
        // The mask the device's memory controller block sets
        // (`gmc_v*_0.c`), or the narrowest of those IP discovery chooses.
        dma_bits: crate::amdgpu::dma_bits,
        // Its table flags only the older APUs it names by ID; IP discovery
        // flags the rest during the probe, after the claim, so the bound
        // device's answer decides.
        integrated: |_| false,
        probed_integrated: Some(crate::amdgpu::query::integrated),
        chip: crate::amdgpu::query::family,
        memory_bytes: crate::amdgpu::query::memory_bytes,
    },
    #[cfg(feature = "metal")]
    metal: Some(crate::metal::amd::ADMISSION),
    #[cfg(all(feature = "metal-linked", not(test)))]
    vulkan: None,
};

/// xe with the display code it shares with i915, and ANV's admission. Its
/// Vulkan driver is not given yet, even in an image that holds ANV: ANV has
/// run on no Intel GPU under Stela. Until then its GPU shows the frames
/// another GPU draws, on the displays wired to it.
#[cfg(feature = "xe-port")]
pub static INTEL: GpuStack = GpuStack {
    vendor: Vendor::Intel,
    kernel: Kernel {
        driver: "xe",
        admits: crate::xe::admits,
        // GTTMMADR (`regs/xe_bars.h`): the registers, then the GTT.
        registers: 0,
        // Firmware marks GTTMMADR prefetchable, and `i915` maps it as it
        // finds it: this port's laptop reports `Region 0: Memory at
        // 8010000000 (64-bit, prefetchable) [size=16M]` under Linux.
        prefetchable_registers: true,
        // The platform's `dma_mask_size` (`xe_pci.c`), which
        // `xe_set_dma_info` sets as the mask.
        dma_bits: crate::xe::dma_bits,
        // The platform's `is_dgfx`.
        integrated: crate::xe::integrated,
        probed_integrated: None,
        chip: crate::xe::query::chip,
        memory_bytes: crate::xe::query::memory_bytes,
    },
    #[cfg(feature = "metal")]
    metal: Some(crate::metal::intel::ADMISSION),
    #[cfg(all(feature = "metal-linked", not(test)))]
    vulkan: None,
};

/// Brings Linux up and registers every kernel driver built into the image,
/// stopping at the first stage that fails. Linux then matches each presented
/// function against the registered drivers' own ID tables, so one image
/// serves every vendor's devices and registration is not per stack.
/// `command_line` is Linux's: space-separated `module.parameter=value` words
/// that set the built-in drivers' parameters before any of them registers.
///
/// # Safety
/// On the canonical Linux owner's original managed task after Initial and
/// SRCU startup have completed, once per environment. Initcalls may sleep.
#[cfg(feature = "linux-drm")]
pub unsafe fn start_kernel(command_line: &str) -> Result<(), stela_linux::owner::Errno> {
    unsafe extern "C" {
        /// The build's one registration image (`build/image.rs`).
        static stela_linux_gpu_image: c_void;
        fn stela_linux_start(
            image: *const c_void,
            command_line: *const u8,
            length: usize,
            registry: *mut *mut c_void,
        ) -> core::ffi::c_int;
    }
    let mut registry = core::ptr::null_mut();
    // SAFETY: the caller's contract; the image is the build's, the registry
    // copies the command line, and its state lasts as long as what it started.
    match unsafe {
        stela_linux_start(
            &raw const stela_linux_gpu_image,
            command_line.as_ptr(),
            command_line.len(),
            &raw mut registry,
        )
    } {
        0 => Ok(()),
        error => Err(error),
    }
}

impl GpuStack {
    /// Whether this image holds the stack's Vulkan driver, so its GPU draws
    /// the Metal device's frames. Otherwise its kernel driver only shows
    /// frames another GPU drew, on the displays wired to it.
    pub fn draws(&self) -> bool {
        #[cfg(all(feature = "metal-linked", not(test)))]
        return self.vulkan.is_some();
        #[cfg(not(all(feature = "metal-linked", not(test))))]
        false
    }

    /// Where `adapter` comes among the GPUs claimed: drawing discrete GPUs
    /// first, then drawing integrated ones, then those that only show
    /// frames, each group in the firmware's order (the sort is stable). The
    /// GPUs [`metal_devices`] prefers then have the first DRM minors, so a
    /// PC numbers them as it would without the others.
    pub fn claim_order(&self, adapter: &Adapter) -> u8 {
        claim_rank(self.draws(), (self.kernel.integrated)(adapter))
    }

    /// The stack for `adapter`: its vendor's, when that stack's kernel
    /// driver holds the device. Otherwise why Stela drives it with none.
    pub fn select(adapter: &Adapter) -> Result<&'static GpuStack, &'static str> {
        let stack = STACKS
            .iter()
            .copied()
            .find(|stack| stack.vendor == adapter.vendor)
            .ok_or_else(|| adapter.unavailable_reason())?;
        (stack.kernel.admits)(adapter)?;
        Ok(stack)
    }
}

/// [`GpuStack::claim_order`] of a GPU that draws or not and is integrated
/// or not.
fn claim_rank(draws: bool, integrated: bool) -> u8 {
    match (draws, integrated) {
        (true, false) => 0,
        (true, true) => 1,
        (false, _) => 2,
    }
}

/// A bound GPU as the choice of Metal devices sees it: only what its stack
/// reports and where the display was taken.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Candidate {
    /// Its stack draws ([`GpuStack::draws`]): it admits the GPU's Vulkan
    /// device as a Metal device and the image holds that driver.
    pub draws: bool,
    /// Its kernel driver's [`Kernel::integrated`].
    pub integrated: bool,
    /// The connected display was taken on one of its outputs.
    pub drives_display: bool,
}

/// What [`metal_devices`] chose, by index into its candidates.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MetalDevices {
    /// The GPUs that serve a Metal device, the PC's Metal device first.
    pub serve: alloc::vec::Vec<usize>,
    /// GPUs that draw but stay display-only, because a more capable GPU
    /// serves Metal.
    pub display_only: alloc::vec::Vec<usize>,
}

/// Which bound GPUs serve Metal devices. They are the drawing GPUs of the
/// most capable kind present: discrete GPUs, with memory and a link of their
/// own, before GPUs built into the processor. When a discrete GPU draws, an
/// integrated one only shows the displays wired to it, as a GPU without a
/// Vulkan driver does, and serves no second Metal device; an integrated GPU
/// alone serves the Metal device. Any drawing GPU feeds any display, since
/// each frame is copied into the buffer of the GPU that drives it, so among
/// equals the GPU that drives the connected display comes first and the
/// others keep the firmware's order.
pub fn metal_devices(candidates: &[Candidate]) -> MetalDevices {
    let drawing = || candidates.iter().enumerate().filter(|(_, gpu)| gpu.draws);
    let discrete = drawing().any(|(_, gpu)| !gpu.integrated);
    let (mut serve, display_only): (alloc::vec::Vec<_>, alloc::vec::Vec<_>) =
        drawing().partition(|(_, gpu)| !discrete || !gpu.integrated);
    serve.sort_by_key(|(_, gpu)| !gpu.drives_display);
    MetalDevices {
        serve: serve.into_iter().map(|(index, _)| index).collect(),
        display_only: display_only.into_iter().map(|(index, _)| index).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::pci::Address;
    use alloc::vec;

    fn candidate(draws: bool, integrated: bool, drives_display: bool) -> Candidate {
        Candidate {
            draws,
            integrated,
            drives_display,
        }
    }

    #[test]
    fn an_integrated_gpu_alone_serves_the_metal_device() {
        let chosen = metal_devices(&[candidate(true, true, true)]);
        assert_eq!(chosen.serve, [0]);
        assert!(chosen.display_only.is_empty());
        // Also when its panel is dark and the display is elsewhere.
        assert_eq!(metal_devices(&[candidate(true, true, false)]).serve, [0]);
    }

    #[test]
    fn a_drawing_discrete_gpu_leaves_the_integrated_one_display_only() {
        // A muxless laptop: the panel on the integrated GPU, which draws too.
        let laptop = [candidate(true, false, false), candidate(true, true, true)];
        let chosen = metal_devices(&laptop);
        assert_eq!(chosen.serve, [0]);
        assert_eq!(chosen.display_only, [1]);
        // In either order of the firmware's enumeration.
        let chosen = metal_devices(&[laptop[1], laptop[0]]);
        assert_eq!(chosen.serve, [1]);
        assert_eq!(chosen.display_only, [0]);
    }

    #[test]
    fn a_gpu_that_draws_nothing_is_neither_served_nor_chosen_against() {
        // A discrete GPU without a Vulkan driver leaves the integrated GPU
        // the Metal device.
        let chosen = metal_devices(&[candidate(false, false, true), candidate(true, true, false)]);
        assert_eq!(chosen.serve, [1]);
        assert!(chosen.display_only.is_empty());
        // A display-only stack is no candidate of either list.
        let chosen = metal_devices(&[candidate(true, false, false), candidate(false, true, true)]);
        assert_eq!(chosen.serve, [0]);
        assert!(chosen.display_only.is_empty());
        assert_eq!(metal_devices(&[]), MetalDevices::default());
    }

    #[test]
    fn among_equals_the_gpu_driving_the_display_comes_first() {
        let desktop = [
            candidate(true, false, false),
            candidate(true, false, false),
            candidate(true, false, true),
            candidate(true, true, false),
        ];
        let chosen = metal_devices(&desktop);
        assert_eq!(chosen.serve, [2, 0, 1]);
        assert_eq!(chosen.display_only, [3]);
        // Without a display taken, the firmware's order stands.
        let headless = vec![candidate(true, false, false); 3];
        assert_eq!(metal_devices(&headless).serve, [0, 1, 2]);
    }

    fn adapter(vendor: u16) -> Adapter {
        Adapter {
            address: Address {
                segment: 0,
                bus: 1,
                device: 0,
                function: 0,
            },
            vendor: Vendor::from_pci(vendor),
            device: 0x1234,
            class: [3, 0, 0],
        }
    }

    #[test]
    fn a_stack_is_selected_by_pci_vendor_alone() {
        let nvidia = GpuStack::select(&adapter(0x10de)).unwrap();
        assert!(core::ptr::eq(nvidia, &NVIDIA));
        assert_eq!(nvidia.kernel.driver, "nouveau");
        for vendor in [
            #[cfg(not(feature = "amdgpu-port"))]
            0x1002,
            #[cfg(not(feature = "xe-port"))]
            0x8086,
            // ASPEED's BMC display and a vendor no stack names.
            0x1a03,
            0x1234,
        ] {
            let adapter = adapter(vendor);
            assert_eq!(
                GpuStack::select(&adapter).err(),
                Some(adapter.unavailable_reason())
            );
        }
    }

    #[cfg(feature = "amdgpu-port")]
    #[test]
    fn amd_is_selected_for_what_amdgpus_probe_takes_and_draws_nothing() {
        // A GPU amdgpu's table does not name, found by its VGA class.
        let mut integrated = adapter(0x1002);
        integrated.device = 0x0001;
        let amd = GpuStack::select(&integrated).unwrap();
        assert!(core::ptr::eq(amd, &AMD));
        assert_eq!(amd.kernel.driver, "amdgpu");
        assert_eq!(amd.kernel.dma_limit(&integrated), (1 << 44) - 1);
        assert!(!amd.draws());
        #[cfg(feature = "metal")]
        assert!(amd.metal.is_some());
        // An audio function of the same vendor is no device of the table.
        let mut audio = adapter(0x1002);
        audio.class = [4, 3, 0];
        assert_eq!(
            GpuStack::select(&audio).err(),
            Some("amdgpu's PCI ID table does not hold this device")
        );
    }

    #[cfg(feature = "xe-port")]
    #[test]
    fn intel_is_selected_only_for_a_device_of_xes_table_and_draws_nothing() {
        // Tiger Lake's UHD Graphics, then an ID no table holds.
        let mut tiger_lake = adapter(0x8086);
        tiger_lake.device = 0x9a49;
        let intel = GpuStack::select(&tiger_lake).unwrap();
        assert!(core::ptr::eq(intel, &INTEL));
        assert_eq!(intel.kernel.driver, "xe");
        assert_eq!(intel.kernel.dma_limit(&tiger_lake), (1 << 39) - 1);
        assert!((intel.kernel.integrated)(&tiger_lake));
        assert!(!intel.draws());
        #[cfg(feature = "metal")]
        assert!(intel.metal.is_some());
        let unknown = adapter(0x8086);
        assert_eq!(
            GpuStack::select(&unknown).err(),
            crate::xe::admits(&unknown).err()
        );
        assert!(crate::xe::admits(&unknown).is_err());
    }

    #[cfg(feature = "xe-port")]
    #[test]
    fn only_intels_register_window_decodes_prefetchably() {
        // GTTMMADR is prefetchable where firmware assigns it; nouveau's and
        // amdgpu's windows are not, so there a prefetchable BAR is refused.
        assert!(INTEL.kernel.prefetchable_registers);
        assert!(!NVIDIA.kernel.prefetchable_registers);
        #[cfg(feature = "amdgpu-port")]
        assert!(!AMD.kernel.prefetchable_registers);
    }

    #[cfg(feature = "xe-port")]
    #[test]
    fn intel_is_selected_for_the_laptops_arc_140t_of_arrow_lake_h() {
        // Arrow Lake H's Arc 140T, this laptop's own GPU at PCI 8086:7d51,
        // which xe's table holds under its Meteor Lake platform.
        let mut arrow_lake_h = adapter(0x8086);
        arrow_lake_h.device = 0x7d51;
        let intel = GpuStack::select(&arrow_lake_h).unwrap();
        assert!(core::ptr::eq(intel, &INTEL));
        assert_eq!(intel.kernel.driver, "xe");
        assert_eq!(intel.kernel.dma_limit(&arrow_lake_h), (1 << 46) - 1);
        assert!((intel.kernel.integrated)(&arrow_lake_h));
    }

    #[test]
    fn each_vendor_has_one_stack_and_a_usable_dma_mask() {
        for (index, stack) in STACKS.iter().enumerate() {
            assert!(
                STACKS[..index]
                    .iter()
                    .all(|other| other.vendor != stack.vendor)
            );
            let adapter = adapter(stack.vendor.pci_id());
            let bits = (stack.kernel.dma_bits)(&adapter);
            assert!((32..=64).contains(&bits));
            assert_eq!(
                stack.kernel.dma_limit(&adapter).count_ones(),
                u32::from(bits)
            );
            assert!(!stack.kernel.driver.is_empty());
        }
    }

    #[test]
    fn claims_put_drawing_discrete_gpus_first_then_integrated_then_display_only() {
        // (draws, integrated) in the firmware's order: an integrated GPU on
        // bus 0, a display-only GPU, then a discrete GPU.
        let mut gpus = vec![(true, true), (false, false), (true, false), (false, true)];
        gpus.sort_by_key(|&(draws, integrated)| claim_rank(draws, integrated));
        assert_eq!(
            gpus,
            [(true, false), (true, true), (false, false), (false, true)]
        );
        // This test binary links no Vulkan driver, so its stacks come last.
        assert_eq!(NVIDIA.claim_order(&adapter(0x10de)), 2);
        assert!(!(NVIDIA.kernel.integrated)(&adapter(0x10de)));
    }

    #[test]
    fn a_stack_draws_only_with_a_vulkan_driver_in_the_image() {
        // This test binary links no Vulkan driver, whatever the features.
        assert!(!NVIDIA.draws());
        #[cfg(feature = "metal")]
        assert!(NVIDIA.metal.is_some());
    }

    #[test]
    fn nouveau_queries_ask_its_getparam_and_keep_refusals() {
        // `struct drm_nouveau_getparam`: the parameter, then its value.
        let mut asked = alloc::vec::Vec::new();
        let mut answer = |request: u32, argument: *mut c_void| {
            // SAFETY: the query passes its own two-word record.
            let words = unsafe { &mut *argument.cast::<[u64; 2]>() };
            asked.push((request, words[0]));
            words[1] = if words[0] == 11 { 0x1b3 } else { 16 << 30 };
            0
        };
        assert_eq!((NVIDIA.kernel.chip)(&mut answer), Some(0x1b3));
        assert_eq!((NVIDIA.kernel.memory_bytes)(&mut answer), Some(16 << 30));
        assert_eq!(asked, [(0xc010_6440, 11), (0xc010_6440, 8)]);
        let mut refuse = |_: u32, _: *mut c_void| -22;
        assert_eq!((NVIDIA.kernel.chip)(&mut refuse), None);
        assert_eq!((NVIDIA.kernel.memory_bytes)(&mut refuse), None);
        let mut empty = |_: u32, _: *mut c_void| 0;
        assert_eq!((NVIDIA.kernel.memory_bytes)(&mut empty), None);
        // nouveau's table answers alone, without a query.
        assert!(NVIDIA.kernel.probed_integrated.is_none());
    }

    #[cfg(feature = "amdgpu-port")]
    #[test]
    fn amdgpus_bound_device_says_whether_it_is_integrated() {
        // amdgpu's `ids_flags` with fusion: what IP discovery found.
        let mut fused = |_: u32, argument: *mut c_void| {
            // SAFETY: the query passes its record, whose answer buffer
            // holds the 18 words it asks for.
            unsafe {
                let record = &*argument.cast::<[u64; 4]>();
                *(record[0] as *mut u64).add(17) = 1;
            }
            0
        };
        let query = AMD.kernel.probed_integrated.unwrap();
        assert_eq!(query(&mut fused), Some(true));
        let mut refuse = |_: u32, _: *mut c_void| -19;
        assert_eq!(query(&mut refuse), None);
    }
}
