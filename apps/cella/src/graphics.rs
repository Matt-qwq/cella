//! Windows graphics instance configuration, applied before wgpu can initialize any driver.
//!
//! Vulkan is never initialized unless `WGPU_BACKEND` asks for it: enumerating Vulkan adapters
//! crashes inside Intel's `igvk64.dll` on some UHD Graphics drivers (#120), and that access
//! violation happens in the driver, where no Rust error handling can catch it.
//!
//! DirectX 12 shaders are compiled with FXC (`d3dcompiler_47.dll`, part of Windows), never with
//! whichever `dxcompiler.dll` the DLL search path happens to find (#339, #354): see
//! [`dx12_compiler`].

use eframe::{
    NativeOptions,
    egui_wgpu::WgpuSetup,
    wgpu::{Backends, DeviceType, Dx12Compiler},
};

/// What the start-up probe learned about one DirectX 12 adapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dx12Adapter {
    /// `Cpu` is WARP (Microsoft's software rasteriser): it works, but every frame is drawn on the
    /// processor, which makes the app lag on older machines.
    pub device_type: DeviceType,
    /// Whether the adapter offers the limits eframe will ask for when it creates the device.
    pub meets_limits: bool,
}

/// The backends to initialize on Windows.
///
/// An explicit `backend_override` (normally `Backends::from_env()`, i.e. `WGPU_BACKEND`) always
/// wins. Otherwise DirectX 12, as long as `dx12_adapters` (what a DirectX 12-only probe found)
/// holds a usable hardware GPU. Without one, OpenGL is the way to the GPU: graphics chips whose
/// driver has no DirectX 12 support still have an OpenGL driver. When DirectX 12 offers only WARP,
/// both stay in the list, so a hardware GL driver wins and WARP is still there on a machine with no
/// GL driver either (a virtual machine): wgpu prefers any GPU over a software adapter. Vulkan is
/// never part of the default (#120).
///
/// **DirectX 12 is the default because OpenGL costs crashes on AMD.** The note on
/// [`configure_backends`] has the creation case; painting is the other one: 2026-11-26 a 112.8 MB
/// workbook (23.6 M cells) took the process down with 0xc0000005 in `atio6axx.dll` about ten seconds
/// after it opened, on the machine whose owner reported the window flashing — the driver struggling
/// before it died. Avoiding it costs memory: with that workbook open the same session used
/// **1,270 MB on DirectX 12 against 1,057 MB on OpenGL** (one run each, same machine, same build).
/// A crash outranks 213 MB, and `WGPU_BACKEND=gl` still gives the lighter backend to anyone who
/// wants it.
pub fn choose_backends(backend_override: Option<Backends>, dx12_adapters: &[Dx12Adapter]) -> Backends {
    if let Some(backends) = backend_override {
        return backends;
    }
    let hardware: Vec<&Dx12Adapter> = dx12_adapters.iter().filter(|a| a.device_type != DeviceType::Cpu).collect();
    if hardware.iter().any(|a| a.meets_limits) {
        Backends::DX12
    } else if !hardware.is_empty() {
        // A GPU whose DirectX 12 device cannot be created: GL is the way to it.
        Backends::GL
    } else if dx12_adapters.is_empty() {
        // No DirectX 12 driver at all.
        Backends::GL
    } else {
        // WARP only: keep both, a hardware GL adapter outranks a software one.
        Backends::GL | Backends::DX12
    }
}

/// The backends for a second start when the first one failed with a graphics error before the
/// app was created (no adapter, no device, no surface): the other one of DirectX 12 and OpenGL.
/// `None` when `WGPU_BACKEND` chose the backends (the user asked for exactly those) or the first
/// start already tried both. Never Vulkan (#120).
pub fn fallback_backends(backend_override: Option<Backends>, first: Backends) -> Option<Backends> {
    if backend_override.is_some() {
        return None;
    }
    let rest = (Backends::DX12 | Backends::GL).difference(first);
    (!rest.is_empty()).then_some(rest)
}

/// The DirectX 12 shader compiler: `env_choice` (normally `Dx12Compiler::from_env()`, i.e.
/// `WGPU_DX12_COMPILER`, for troubleshooting) or else FXC.
///
/// wgpu's default (`Auto`) loads the first `dxcompiler.dll` on the DLL search path. Cella ships
/// none, so that is another program's copy (a Windows SDK, an Edge or WebView folder, anything on
/// `PATH`). Without a matching `dxil.dll` beside it, DXC prints "DXIL signing library not found",
/// wgpu treats that warning as a compile error, its internal compute pipelines fail, the device is
/// lost and the app quits before the first frame ("Parent device is lost"). FXC is part of Windows
/// and needs no extra DLL; egui's shaders don't need anything newer.
pub fn dx12_compiler(env_choice: Option<Dx12Compiler>) -> Dx12Compiler {
    env_choice.unwrap_or(Dx12Compiler::Fxc)
}

/// Restricts the wgpu instance eframe creates to `backends`, and pins its DirectX 12 shader
/// compiler ([`dx12_compiler`]).
pub fn configure_backends(options: &mut NativeOptions, backends: Backends) {
    if let WgpuSetup::CreateNew(create) = &mut options.wgpu_options.wgpu_setup {
        create.instance_descriptor.backend_options.dx12.shader_compiler = dx12_compiler(Dx12Compiler::from_env());
        // eframe's default includes Vulkan and GL. Creating the GL backend can crash inside AMD's
        // atio6axx.dll — painting with it can too (the workbook of 2026-11-26 saved by the default
        // in `choose_backends`) — and enumerating Vulkan adapters inside Intel's igvk64.dll (#120), before
        // adapter selection or any Rust error handling runs, so the window never appears.
        // Choosing an adapter afterwards is too late: only leaving the backend out of the
        // instance avoids it. This matches the verified WGPU_BACKEND=dx12 workaround without
        // mutating process environment variables.
        create.instance_descriptor.backends = backends;
    }
}

/// Chooses the backends ([`choose_backends`]), applies them to `options` and returns them.
/// Without a `backend_override` this first probes DirectX 12 with a short-lived instance of its
/// own, which is dropped before eframe creates the real one.
#[cfg(target_os = "windows")]
pub fn configure(options: &mut NativeOptions, backend_override: Option<Backends>) -> Backends {
    let backends = match backend_override {
        Some(backends) => {
            log::info!("graphics: WGPU_BACKEND override, backends {backends:?}");
            backends
        }
        None => {
            let adapters = probe_dx12(options);
            let backends = choose_backends(None, &adapters);
            log::info!("graphics: {} DirectX 12 adapter(s) {adapters:?}, backends {backends:?}", adapters.len());
            backends
        }
    };
    configure_backends(options, backends);
    backends
}

/// Lists the DirectX 12 adapters, checking each against the limits eframe's device descriptor
/// will require. Empty when this build has no DirectX 12 backend.
#[cfg(target_os = "windows")]
fn probe_dx12(options: &NativeOptions) -> Vec<Dx12Adapter> {
    use eframe::wgpu::{Instance, InstanceDescriptor};
    // `Instance::new` panics when the build has no backend for the platform.
    if !Instance::enabled_backend_features().contains(Backends::DX12) {
        return Vec::new();
    }
    let WgpuSetup::CreateNew(create) = &options.wgpu_options.wgpu_setup else {
        return Vec::new();
    };
    let instance = Instance::new(InstanceDescriptor { backends: Backends::DX12, ..InstanceDescriptor::new_without_display_handle() });
    pollster::block_on(instance.enumerate_adapters(Backends::DX12))
        .iter()
        .map(|adapter| {
            let info = adapter.get_info();
            let required = (create.device_descriptor)(adapter).required_limits;
            let meets_limits = required.check_limits(&adapter.limits());
            log::info!("graphics: DirectX 12 adapter {:?} ({:?}, driver {:?}), limits ok: {meets_limits}", info.name, info.device_type, info.driver);
            Dx12Adapter { device_type: info.device_type, meets_limits }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn configured_backends(backends: Backends) -> Backends {
        let mut options = NativeOptions::default();
        configure_backends(&mut options, backends);
        let WgpuSetup::CreateNew(create) = options.wgpu_options.wgpu_setup else {
            panic!("default native options must create a wgpu instance");
        };
        create.instance_descriptor.backends
    }

    const fn adapter(device_type: DeviceType, meets_limits: bool) -> Dx12Adapter {
        Dx12Adapter { device_type, meets_limits }
    }

    #[test]
    fn windows_default_never_initializes_vulkan() {
        // eframe's own default (PRIMARY | GL) would initialize Vulkan and OpenGL as well.
        let WgpuSetup::CreateNew(eframe_default) = NativeOptions::default().wgpu_options.wgpu_setup else {
            panic!("default native options must create a wgpu instance");
        };
        assert!(eframe_default.instance_descriptor.backends.contains(Backends::GL));
        let gpu = [adapter(DeviceType::IntegratedGpu, true)];
        assert_eq!(configured_backends(choose_backends(None, &gpu)), Backends::DX12);
        // #120: whatever the DirectX 12 probe finds, and whatever the retry picks, no Vulkan.
        let probes: [&[Dx12Adapter]; 4] = [&[], &gpu, &[adapter(DeviceType::Cpu, true)], &[adapter(DeviceType::IntegratedGpu, false)]];
        for adapters in probes {
            let first = choose_backends(None, adapters);
            assert!(!first.contains(Backends::VULKAN), "{adapters:?}");
            if let Some(second) = fallback_backends(None, first) {
                assert!(!second.contains(Backends::VULKAN), "{adapters:?}");
            }
        }
    }

    #[test]
    fn explicit_backend_override_is_preserved() {
        let warp_only = [adapter(DeviceType::Cpu, true)];
        for backends in [Backends::VULKAN, Backends::GL, Backends::DX12, Backends::VULKAN | Backends::DX12, Backends::empty()] {
            // Whatever the probe would have found, the override wins (the probe doesn't run then),
            // and a failed start isn't retried with something the user didn't ask for.
            assert_eq!(choose_backends(Some(backends), &[]), backends);
            assert_eq!(choose_backends(Some(backends), &warp_only), backends);
            assert_eq!(configured_backends(backends), backends);
            assert_eq!(fallback_backends(Some(backends), backends), None);
        }
    }

    #[test]
    fn dx12_when_the_probe_finds_a_usable_gpu() {
        // A hardware GPU DirectX 12 can use: DirectX 12, which is also the backend the OpenGL
        // driver crashes under on AMD.
        let gpu = [adapter(DeviceType::IntegratedGpu, true)];
        assert_eq!(choose_backends(None, &gpu), Backends::DX12);
        let mixed = [adapter(DeviceType::Cpu, true), adapter(DeviceType::IntegratedGpu, false), adapter(DeviceType::DiscreteGpu, true)];
        assert_eq!(choose_backends(None, &mixed), Backends::DX12);
        // No DirectX 12 driver: GL is the way to the GPU; WARP-only keeps both.
        let warp_only = [adapter(DeviceType::Cpu, true)];
        assert_eq!(choose_backends(None, &warp_only), Backends::GL | Backends::DX12);
        assert_eq!(choose_backends(None, &[]), Backends::GL);
        // A GPU whose DirectX 12 device can't be created: GL alone.
        let weak_gpu = [adapter(DeviceType::IntegratedGpu, false), adapter(DeviceType::Cpu, true)];
        assert_eq!(choose_backends(None, &weak_gpu), Backends::GL);
        // The retry path offers the backend the first start did not try; a list that already
        // holds both has nothing left.
        for adapters in [&[][..], &warp_only[..], &weak_gpu[..], &mixed[..], &gpu[..]] {
            let first = choose_backends(None, adapters);
            let expected = match first {
                Backends::GL => Some(Backends::DX12),
                Backends::DX12 => Some(Backends::GL),
                _ => None,
            };
            assert_eq!(fallback_backends(None, first), expected, "{adapters:?}");
        }
    }

    #[test]
    fn failed_start_retries_with_the_other_backend() {
        assert_eq!(fallback_backends(None, Backends::DX12), Some(Backends::GL));
        assert_eq!(fallback_backends(None, Backends::GL), Some(Backends::DX12));
        // Both were already tried: nothing left to fall back to.
        assert_eq!(fallback_backends(None, Backends::DX12 | Backends::GL), None);
    }

    #[test]
    fn dx12_compiles_shaders_with_fxc_unless_asked() {
        // #339, #354: wgpu's `Auto` would load a stray dxcompiler.dll and lose the device.
        assert!(matches!(dx12_compiler(None), Dx12Compiler::Fxc));
        assert!(matches!(dx12_compiler(Some(Dx12Compiler::Auto)), Dx12Compiler::Auto));
        assert!(matches!(dx12_compiler(Some(Dx12Compiler::default_dynamic_dxc())), Dx12Compiler::DynamicDxc { .. }));
        // eframe's own default is `Auto`; the configured instance (first start and retry) uses FXC.
        let WgpuSetup::CreateNew(eframe_default) = NativeOptions::default().wgpu_options.wgpu_setup else {
            panic!("default native options must create a wgpu instance");
        };
        if Dx12Compiler::from_env().is_none() {
            assert!(matches!(eframe_default.instance_descriptor.backend_options.dx12.shader_compiler, Dx12Compiler::Auto));
            let mut options = NativeOptions::default();
            configure_backends(&mut options, Backends::DX12);
            let WgpuSetup::CreateNew(create) = options.wgpu_options.wgpu_setup else {
                panic!("default native options must create a wgpu instance");
            };
            assert!(matches!(create.instance_descriptor.backend_options.dx12.shader_compiler, Dx12Compiler::Fxc));
        }
    }
}
