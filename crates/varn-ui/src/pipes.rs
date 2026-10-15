use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::OnceLock;

const RECT_WGSL: &str = r#"
struct U { vp: vec2<f32> };
@group(0) @binding(0) var<uniform> u: U;
struct VIn {
    @location(0) pos: vec2<f32>,
    @location(1) size: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) radius: f32,
};
struct VOut {
    @builtin(position) p: vec4<f32>,
    @location(0) c: vec4<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) sz: vec2<f32>,
    @location(3) rad: f32,
};
@vertex fn vs(@builtin(vertex_index) vi: u32, v: VIn) -> VOut {
    var c = array<vec2<f32>, 6>(
        vec2(0., 0.), vec2(1., 0.), vec2(0., 1.),
        vec2(0., 1.), vec2(1., 0.), vec2(1., 1.));
    let q = c[vi];
    let px = v.pos + q * v.size;
    var o: VOut;
    o.p = vec4(px.x / u.vp.x * 2. - 1., 1. - px.y / u.vp.y * 2., 0., 1.);
    o.c = v.color;
    o.uv = q;
    o.sz = v.size;
    o.rad = v.radius;
    return o;
}
fn sd_round_box(p: vec2<f32>, b: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - b + r;
    return length(max(q, vec2(0.))) + min(max(q.x, q.y), 0.) - r;
}
@fragment fn fs(v: VOut) -> @location(0) vec4<f32> {
    let d = sd_round_box((v.uv - 0.5) * v.sz, v.sz * 0.5, v.rad);
    let a = 1. - smoothstep(-1., 1., d);
    return vec4(v.c.rgb, v.c.a * a);
}
"#;

const TEXT_WGSL: &str = r#"
struct U { vp: vec2<f32> };
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var t: texture_2d<f32>;
@group(0) @binding(2) var s: sampler;
struct VIn {
    @location(0) pos: vec2<f32>,
    @location(1) size: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) uvpos: vec2<f32>,
    @location(4) uvsize: vec2<f32>,
};
struct VOut {
    @builtin(position) p: vec4<f32>,
    @location(0) c: vec4<f32>,
    @location(1) uv: vec2<f32>,
};
@vertex fn vs(@builtin(vertex_index) vi: u32, v: VIn) -> VOut {
    var c = array<vec2<f32>, 6>(
        vec2(0., 0.), vec2(1., 0.), vec2(0., 1.),
        vec2(0., 1.), vec2(1., 0.), vec2(1., 1.));
    let q = c[vi];
    let px = v.pos + q * v.size;
    var o: VOut;
    o.p = vec4(px.x / u.vp.x * 2. - 1., 1. - px.y / u.vp.y * 2., 0., 1.);
    o.c = v.color;
    o.uv = v.uvpos + q * v.uvsize;
    return o;
}
@fragment fn fs(v: VOut) -> @location(0) vec4<f32> {
    return vec4(v.c.rgb, v.c.a * textureSample(t, s, v.uv).r);
}
"#;

struct Pipes {
    rect: std::sync::Arc<wgpu::RenderPipeline>,
    text: std::sync::Arc<wgpu::RenderPipeline>,
    rect_layout: std::sync::Arc<wgpu::BindGroupLayout>,
    text_layout: std::sync::Arc<wgpu::BindGroupLayout>,
}

static PIPES: OnceLock<Mutex<HashMap<wgpu::TextureFormat, Pipes>>> = OnceLock::new();

fn pipes() -> &'static Mutex<HashMap<wgpu::TextureFormat, Pipes>> {
    PIPES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn entry(binding: u32, ty: wgpu::BufferBindingType) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX,
        ty: wgpu::BindingType::Buffer {
            ty,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn inst_attrs(extra: usize) -> (Vec<wgpu::VertexAttribute>, u64) {
    let mut attrs = vec![
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x2,
            offset: 0,
            shader_location: 0,
        },
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x2,
            offset: 8,
            shader_location: 1,
        },
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x4,
            offset: 16,
            shader_location: 2,
        },
    ];
    for i in 0..extra {
        attrs.push(wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x2,
            offset: (32 + i * 8) as u64,
            shader_location: (3 + i) as u32,
        });
    }
    let stride: u64 = if extra == 0 { 32 } else { 48 };
    (attrs, stride)
}

pub fn get_pipes(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
) -> (
    std::sync::Arc<wgpu::RenderPipeline>,
    std::sync::Arc<wgpu::RenderPipeline>,
    std::sync::Arc<wgpu::BindGroupLayout>,
    std::sync::Arc<wgpu::BindGroupLayout>,
) {
    {
        let cache = pipes().lock();
        if let Some(p) = cache.get(&format) {
            return (
                p.rect.clone(),
                p.text.clone(),
                p.rect_layout.clone(),
                p.text_layout.clone(),
            );
        }
    }
    let rect_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[entry(0, wgpu::BufferBindingType::Uniform)],
    });
    let text_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[
            entry(0, wgpu::BufferBindingType::Uniform),
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    let mk = |layout: &wgpu::BindGroupLayout,
              src: &str,
              attrs: &[wgpu::VertexAttribute],
              stride: u64| {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(src.into()),
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[layout],
            push_constant_ranges: &[],
        });
        let bufs = [wgpu::VertexBufferLayout {
            array_stride: stride,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: attrs,
        }];
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: "vs",
                buffers: &bufs,
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: "fs",
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        })
    };
    let (mut rattrs, _) = inst_attrs(0);
    rattrs.push(wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32,
        offset: 32,
        shader_location: 3,
    });
    let rstride = 36u64;
    let (tattrs, tstride) = inst_attrs(2);
    let rect = mk(&rect_layout, RECT_WGSL, &rattrs, rstride);
    let text = mk(&text_layout, TEXT_WGSL, &tattrs, tstride);
    let rect = std::sync::Arc::new(rect);
    let text = std::sync::Arc::new(text);
    let rect_layout = std::sync::Arc::new(rect_layout);
    let text_layout = std::sync::Arc::new(text_layout);
    let mut cache = pipes().lock();
    cache.insert(
        format,
        Pipes {
            rect: rect.clone(),
            text: text.clone(),
            rect_layout: rect_layout.clone(),
            text_layout: text_layout.clone(),
        },
    );
    (rect, text, rect_layout, text_layout)
}
