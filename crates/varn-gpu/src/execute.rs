use std::collections::HashMap;
use std::sync::OnceLock;

use crate::arena::Handle;
use crate::Op;

const WG: u32 = 64;
const SHADER_ADD: &str = r#"
@group(0) @binding(0) var<storage, read> a: array<f32>;
@group(0) @binding(1) var<storage, read> b: array<f32>;
@group(0) @binding(2) var<storage, read_write> o: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i < arrayLength(&o)) { o[i] = a[i] + b[i]; }
}
"#;

const SHADER_MUL: &str = r#"
@group(0) @binding(0) var<storage, read> a: array<f32>;
@group(0) @binding(1) var<storage, read> b: array<f32>;
@group(0) @binding(2) var<storage, read_write> o: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i < arrayLength(&o)) { o[i] = a[i] * b[i]; }
}
"#;

const SHADER_SCALE: &str = r#"
@group(0) @binding(0) var<storage, read> a: array<f32>;
@group(0) @binding(1) var<storage, read> u: array<f32>;
@group(0) @binding(2) var<storage, read_write> o: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i < arrayLength(&o)) { o[i] = a[i] * u[0]; }
}
"#;

const SHADER_MATMUL: &str = r#"
@group(0) @binding(0) var<storage, read> a: array<f32>;
@group(0) @binding(1) var<storage, read> b: array<f32>;
@group(0) @binding(2) var<storage, read> u: array<u32>;
@group(0) @binding(3) var<storage, read_write> o: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let m = u[0];
    let k = u[1];
    let n = u[2];
    let total = m * n;
    let t = gid.x;
    if (t >= total) { return; }
    let i = t / n;
    let j = t % n;
    var acc = 0.0;
    for (var p = 0u; p < k; p = p + 1u) {
        acc = acc + a[i * k + p] * b[p * n + j];
    }
    o[t] = acc;
}
"#;

pub struct Pipes {
    pub add: wgpu::ComputePipeline,
    pub mul: wgpu::ComputePipeline,
    pub scale: wgpu::ComputePipeline,
    pub matmul: wgpu::ComputePipeline,
}

pub struct Ctx {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub pipes: Pipes,
    pub layout3: wgpu::BindGroupLayout,
    pub layout4: wgpu::BindGroupLayout,
}

fn layout(device: &wgpu::Device, n: u32) -> wgpu::BindGroupLayout {
    let mut entries = Vec::new();
    for b in 0..n {
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: b,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage {
                    read_only: b != n - 1,
                },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        });
    }
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &entries,
    })
}

fn pipe(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, src: &str) -> wgpu::ComputePipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(src.into()),
    });
    let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[layout],
        push_constant_ranges: &[],
    });
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&pl),
        module: &module,
        entry_point: "main",
        compilation_options: Default::default(),
        cache: None,
    })
}

pub struct Job {
    pub handle: Handle,
    pub op: Op,
    pub inputs: Vec<Handle>,
    pub len: usize,
}

fn storage(c: &Ctx, data: &[u8], out: bool) -> wgpu::Buffer {
    let usage = wgpu::BufferUsages::STORAGE
        | if out {
            wgpu::BufferUsages::COPY_SRC
        } else {
            wgpu::BufferUsages::COPY_DST
        };
    let buf = c.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: data.len().max(4) as u64,
        usage,
        mapped_at_creation: false,
    });
    if !out && !data.is_empty() {
        c.queue.write_buffer(&buf, 0, data);
    }
    buf
}

fn bind(c: &Ctx, layout: &wgpu::BindGroupLayout, bufs: &[&wgpu::Buffer]) -> wgpu::BindGroup {
    let entries: Vec<wgpu::BindGroupEntry> = bufs
        .iter()
        .enumerate()
        .map(|(i, b)| wgpu::BindGroupEntry {
            binding: i as u32,
            resource: b.as_entire_binding(),
        })
        .collect();
    c.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout,
        entries: &entries,
    })
}

pub fn run(
    c: &Ctx,
    jobs: &[Job],
    leaves: &HashMap<Handle, Vec<f32>>,
) -> Result<HashMap<Handle, Vec<f32>>, String> {
    let mut dev: HashMap<Handle, wgpu::Buffer> = HashMap::new();
    for (h, data) in leaves {
        if data.is_empty() {
            continue;
        }
        dev.insert(*h, storage(c, bytemuck::cast_slice(data), false));
    }
    let mut enc = c
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    let mut staged: Vec<(Handle, wgpu::Buffer)> = Vec::new();
    for job in jobs {
        if job.len == 0 {
            continue;
        }
        let ins: Vec<&wgpu::Buffer> = job
            .inputs
            .iter()
            .map(|h| {
                dev.get(h)
                    .ok_or_else(|| "gpu: missing input buffer".to_string())
            })
            .collect::<Result<_, _>>()?;
        let (pipeline, layout, extra) = match job.op {
            Op::Add => (&c.pipes.add, &c.layout3, None),
            Op::Mul => (&c.pipes.mul, &c.layout3, None),
            Op::Scale { factor } => {
                let fb = storage(c, bytemuck::cast_slice(&[factor]), false);
                (&c.pipes.scale, &c.layout3, Some(fb))
            }
            Op::MatMul { m, k, n } => {
                let dims: [u32; 4] = [m as u32, k as u32, n as u32, 0];
                let db = storage(c, bytemuck::cast_slice(&dims), false);
                (&c.pipes.matmul, &c.layout4, Some(db))
            }
        };
        let out = storage(c, &vec![0u8; job.len * 4], true);
        let mut refs: Vec<&wgpu::Buffer> = ins;
        let owned;
        if let Some(b) = extra {
            owned = b;
            refs.push(&owned);
        }
        refs.push(&out);
        let group = bind(c, layout, &refs);
        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: None,
                timestamp_writes: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups((job.len as u32).div_ceil(WG), 1, 1);
        }
        let stage = c.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (job.len * 4) as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        enc.copy_buffer_to_buffer(&out, 0, &stage, 0, (job.len * 4) as u64);
        staged.push((job.handle, stage));
        dev.insert(job.handle, out);
    }
    c.queue.submit(Some(enc.finish()));
    let mut result = HashMap::new();
    for job in jobs {
        if job.len == 0 {
            result.insert(job.handle, Vec::new());
        }
    }
    for (h, stage) in &staged {
        let (tx, rx) = std::sync::mpsc::channel();
        stage.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            tx.send(r).ok();
        });
        c.device.poll(wgpu::Maintain::Wait);
        rx.recv()
            .map_err(|_| "gpu: map channel closed".to_string())?
            .map_err(|e| format!("gpu: map failed: {e:?}"))?;
        {
            let view = stage.slice(..).get_mapped_range();
            let vals: &[f32] = bytemuck::cast_slice(&view);
            result.insert(*h, vals.to_vec());
        }
        stage.unmap();
    }
    Ok(result)
}

static CTX: OnceLock<Option<Ctx>> = OnceLock::new();

pub fn ctx() -> Option<&'static Ctx> {
    CTX.get_or_init(|| {
        let r = std::panic::catch_unwind(init);
        r.unwrap_or(None)
    })
    .as_ref()
}

fn init() -> Option<Ctx> {
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
    let layout3 = layout(&device, 3);
    let layout4 = layout(&device, 4);
    Some(Ctx {
        pipes: Pipes {
            add: pipe(&device, &layout3, SHADER_ADD),
            mul: pipe(&device, &layout3, SHADER_MUL),
            scale: pipe(&device, &layout3, SHADER_SCALE),
            matmul: pipe(&device, &layout4, SHADER_MATMUL),
        },
        device,
        queue,
        layout3,
        layout4,
    })
}
