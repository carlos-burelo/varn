use std::sync::OnceLock;

pub struct AdapterDesc {
    pub name: String,
    pub backend: &'static str,
    pub discrete: bool,
    pub integrated: bool,
}

pub struct Probe {
    pub adapters: Vec<AdapterDesc>,
    pub best: Option<usize>,
}

fn backend_str(b: wgpu::Backend) -> &'static str {
    match b {
        wgpu::Backend::Vulkan => "vulkan",
        wgpu::Backend::Metal => "metal",
        wgpu::Backend::Dx12 => "dx12",
        wgpu::Backend::Gl => "gl",
        wgpu::Backend::BrowserWebGpu => "webgpu",
        wgpu::Backend::Empty => "empty",
    }
}

fn run_probe() -> Probe {
    let caught = std::panic::catch_unwind(|| {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });
        let mut adapters: Vec<AdapterDesc> = instance
            .enumerate_adapters(wgpu::Backends::all())
            .iter()
            .map(|a| {
                let info = a.get_info();
                AdapterDesc {
                    name: info.name,
                    backend: backend_str(info.backend),
                    discrete: matches!(info.device_type, wgpu::DeviceType::DiscreteGpu),
                    integrated: matches!(info.device_type, wgpu::DeviceType::IntegratedGpu),
                }
            })
            .collect();
        adapters.sort_by_key(|a| {
            if a.discrete {
                0u8
            } else if a.integrated {
                1u8
            } else {
                2u8
            }
        });
        adapters
    });
    let adapters = caught.unwrap_or_default();
    let best = if adapters.is_empty() { None } else { Some(0) };
    Probe { adapters, best }
}

static PROBE: OnceLock<Probe> = OnceLock::new();

pub fn probe() -> &'static Probe {
    PROBE.get_or_init(run_probe)
}

pub fn backend_name() -> &'static str {
    let p = probe();
    match p.best.and_then(|i| p.adapters.get(i)) {
        Some(a) => a.backend,
        None => "cpu-graph",
    }
}

pub fn device_count() -> i64 {
    probe().adapters.len() as i64
}

pub fn adapter_name(i: usize) -> Option<&'static str> {
    probe().adapters.get(i).map(|a| a.name.as_str())
}

pub fn is_discrete(i: usize) -> bool {
    probe().adapters.get(i).is_some_and(|a| a.discrete)
}
