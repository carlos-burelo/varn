use std::sync::{Arc, OnceLock};

pub struct Shared {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

static SHARED: OnceLock<Option<Arc<Shared>>> = OnceLock::new();

pub fn shared() -> Option<Arc<Shared>> {
    SHARED
        .get_or_init(|| {
            let r = std::panic::catch_unwind(init);
            r.unwrap_or(None).map(Arc::new)
        })
        .clone()
}

fn init() -> Option<Shared> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::all(),
        ..Default::default()
    });
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: false,
        compatible_surface: None,
    }))?;
    let (device, queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: None,
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: Default::default(),
        },
        None,
    ))
    .ok()?;
    Some(Shared {
        instance,
        adapter,
        device,
        queue,
    })
}

pub fn make_surface(
    shared: &Shared,
    window: &winit::window::Window,
) -> Option<wgpu::Surface<'static>> {
    let surface = shared.instance.create_surface(window).ok()?;
    Some(unsafe { std::mem::transmute::<wgpu::Surface<'_>, wgpu::Surface<'static>>(surface) })
}
pub fn configure_surface(
    shared: &Shared,
    surface: &wgpu::Surface,
    w: u32,
    h: u32,
    configured: &mut Option<(u32, u32)>,
) -> Option<(wgpu::SurfaceConfiguration, wgpu::TextureFormat)> {
    let caps = surface.get_capabilities(&shared.adapter);
    if caps.formats.is_empty() || caps.alpha_modes.is_empty() {
        return None;
    }
    let format = caps
        .formats
        .iter()
        .copied()
        .find(|f| f.is_srgb())
        .unwrap_or(caps.formats[0]);
    let config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format,
        width: w.max(1),
        height: h.max(1),
        present_mode: wgpu::PresentMode::AutoVsync,
        alpha_mode: caps.alpha_modes[0],
        view_formats: vec![],
        desired_maximum_frame_latency: 2,
    };
    if *configured != Some((config.width, config.height)) {
        surface.configure(&shared.device, &config);
        *configured = Some((config.width, config.height));
    }
    Some((config, format))
}
