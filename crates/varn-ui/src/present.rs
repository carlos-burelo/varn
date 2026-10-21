use super::pipes::get_pipes;
use super::quad::{Glyph, Rect};
use super::render::{self, Shared};

fn upload(device: &wgpu::Device, queue: &wgpu::Queue, data: &[f32]) -> wgpu::Buffer {
    upload_with(
        device,
        queue,
        data,
        wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
    )
}

fn upload_uniform(device: &wgpu::Device, queue: &wgpu::Queue, data: &[f32]) -> wgpu::Buffer {
    upload_with(
        device,
        queue,
        data,
        wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    )
}

fn upload_with(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    data: &[f32],
    usage: wgpu::BufferUsages,
) -> wgpu::Buffer {
    let bytes: &[u8] = bytemuck::cast_slice(data);
    let buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: bytes.len().max(4) as u64,
        usage,
        mapped_at_creation: false,
    });
    if !data.is_empty() {
        queue.write_buffer(&buf, 0, bytes);
    }
    buf
}

pub fn present_draw(
    shared: &Shared,
    surface: &wgpu::Surface,
    w: u32,
    h: u32,
    configured: &mut Option<(u32, u32)>,
    clear: [f32; 4],
    rects: &[Rect],
    glyphs: &[Glyph],
    atlas: Option<(&[u8], u32, u32)>,
) -> bool {
    let Some((_config, format)) = render::configure_surface(shared, surface, w, h, configured)
    else {
        return false;
    };
    let frame = match surface.get_current_texture() {
        Ok(f) => f,
        Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
            *configured = None;
            let Some((_config, _)) = render::configure_surface(shared, surface, w, h, configured)
            else {
                return false;
            };
            match surface.get_current_texture() {
                Ok(f) => f,
                Err(_) => return false,
            }
        }
        Err(_) => return false,
    };
    let look = frame
        .texture
        .create_view(&wgpu::TextureViewDescriptor::default());
    let mut enc = shared
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    if rects.is_empty() && glyphs.is_empty() {
        clear_only(&mut enc, &look, clear);
    } else {
        let tex = atlas.and_then(|(bytes, aw, ah)| {
            if glyphs.is_empty() {
                return None;
            }
            let size = wgpu::Extent3d {
                width: aw.max(1),
                height: ah.max(1),
                depth_or_array_layers: 1,
            };
            let t = shared.device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            shared.queue.write_texture(
                wgpu::ImageCopyTexture {
                    texture: &t,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                bytes,
                wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(aw.max(1)),
                    rows_per_image: Some(ah.max(1)),
                },
                size,
            );
            let view = t.create_view(&wgpu::TextureViewDescriptor::default());
            let sampler = shared.device.create_sampler(&wgpu::SamplerDescriptor {
                label: None,
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                address_mode_w: wgpu::AddressMode::ClampToEdge,
                mag_filter: wgpu::FilterMode::Nearest,
                min_filter: wgpu::FilterMode::Nearest,
                mipmap_filter: wgpu::FilterMode::Nearest,
                ..Default::default()
            });
            Some((view, sampler))
        });
        let tex_ref = tex.as_ref().map(|(v, s)| (v, s));
        draw_scene(
            shared, format, &look, &mut enc, w as f32, h as f32, clear, tex_ref, rects, glyphs,
        );
        let _ = tex;
    }
    shared.queue.submit(Some(enc.finish()));
    frame.present();
    true
}

fn clear_only(enc: &mut wgpu::CommandEncoder, look: &wgpu::TextureView, clear: [f32; 4]) {
    let _pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: None,
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: look,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color {
                    r: clear[0] as f64,
                    g: clear[1] as f64,
                    b: clear[2] as f64,
                    a: clear[3] as f64,
                }),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    });
}

fn draw_scene(
    shared: &Shared,
    format: wgpu::TextureFormat,
    view: &wgpu::TextureView,
    enc: &mut wgpu::CommandEncoder,
    vw: f32,
    vh: f32,
    clear: [f32; 4],
    atlas: Option<(&wgpu::TextureView, &wgpu::Sampler)>,
    rects: &[Rect],
    glyphs: &[Glyph],
) {
    let (rect_pipe, text_pipe, rect_layout, text_layout) = get_pipes(&shared.device, format);
    let uni = upload_uniform(&shared.device, &shared.queue, &[vw, vh]);
    let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: None,
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color {
                    r: clear[0] as f64,
                    g: clear[1] as f64,
                    b: clear[2] as f64,
                    a: clear[3] as f64,
                }),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    });
    if !rects.is_empty() {
        let mut data = Vec::with_capacity(rects.len() * 9);
        for r in rects {
            data.extend_from_slice(&[
                r.x, r.y, r.w, r.h, r.color[0], r.color[1], r.color[2], r.color[3], r.radius,
            ]);
        }
        let buf = upload(&shared.device, &shared.queue, &data);
        let group = shared.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &rect_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uni.as_entire_binding(),
            }],
        });
        pass.set_pipeline(&rect_pipe);
        pass.set_bind_group(0, &group, &[]);
        pass.set_vertex_buffer(0, buf.slice(..));
        pass.draw(0..6, 0..rects.len() as u32);
    }
    if !glyphs.is_empty() {
        let Some((tex_view, sampler)) = atlas else {
            return;
        };
        let mut data = Vec::with_capacity(glyphs.len() * 12);
        for g in glyphs {
            data.extend_from_slice(&[
                g.x, g.y, g.w, g.h, g.color[0], g.color[1], g.color[2], g.color[3], g.u, g.v, g.uw,
                g.vh,
            ]);
        }
        let buf = upload(&shared.device, &shared.queue, &data);
        let group = shared.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &text_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uni.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(tex_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        });
        pass.set_pipeline(&text_pipe);
        pass.set_bind_group(0, &group, &[]);
        pass.set_vertex_buffer(0, buf.slice(..));
        pass.draw(0..6, 0..glyphs.len() as u32);
    }
}
