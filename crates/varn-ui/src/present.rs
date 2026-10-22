use std::sync::Arc;

use egui_wgpu::{Renderer, RendererOptions, ScreenDescriptor};
use epaint::{
    emath::{pos2, vec2},
    textures::TextureOptions,
    ClippedPrimitive, Color32, ColorImage, ImageData, ImageDelta, Mesh, Primitive, Rect as ERect,
    Shape, TessellationOptions, Tessellator, TextureId, Vertex,
};
use parking_lot::Mutex;
use rustc_hash::FxHashMap;

use super::quad::{Body, Clip, Glyph, Rect, Run};
use super::render::{configure_surface, Shared};

const WHITE_TEXTURE: TextureId = TextureId::Managed(0);
const PIXELS_PER_POINT: f32 = 1.0;
const MAX_CORNER: f32 = 255.0;

struct Painter {
    renderer: Renderer,
}

static PAINTERS: Mutex<Option<FxHashMap<wgpu::TextureFormat, Painter>>> = Mutex::new(None);

fn with_painter<T>(
    shared: &Shared,
    format: wgpu::TextureFormat,
    f: impl FnOnce(&mut Painter) -> T,
) -> T {
    let mut guard = PAINTERS.lock();
    let map = guard.get_or_insert_with(FxHashMap::default);
    let painter = map.entry(format).or_insert_with(|| {
        let mut renderer = Renderer::new(&shared.device, format, RendererOptions::default());
        let white = ColorImage::new([1, 1], vec![Color32::WHITE]);
        renderer.update_texture(
            &shared.device,
            &shared.queue,
            WHITE_TEXTURE,
            &ImageDelta::full(ImageData::Color(Arc::new(white)), TextureOptions::NEAREST),
        );
        Painter { renderer }
    });
    f(painter)
}

fn color(rgba: [f32; 4]) -> Color32 {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgba_unmultiplied(byte(rgba[0]), byte(rgba[1]), byte(rgba[2]), byte(rgba[3]))
}

fn clip_rect(clip: Option<Clip>, vw: f32, vh: f32) -> ERect {
    match clip {
        Some(c) => ERect::from_min_size(pos2(c.x, c.y), vec2(c.w, c.h)),
        None => ERect::from_min_size(pos2(0.0, 0.0), vec2(vw, vh)),
    }
}

fn corner(radius: f32) -> u8 {
    radius.round().clamp(0.0, MAX_CORNER) as u8
}

fn rect_mesh(tess: &mut Tessellator, rects: &[Rect]) -> Mesh {
    let mut mesh = Mesh::default();
    for r in rects {
        let shape = Shape::rect_filled(
            ERect::from_min_size(pos2(r.x, r.y), vec2(r.w, r.h)),
            corner(r.radius),
            color(r.color),
        );
        tess.tessellate_shape(shape, &mut mesh);
    }
    mesh
}

fn glyph_mesh(texture: TextureId, glyphs: &[Glyph]) -> Mesh {
    let mut mesh = Mesh::with_texture(texture);
    for g in glyphs {
        let base = mesh.vertices.len() as u32;
        let tint = color(g.color);
        let corners = [
            (g.x, g.y, g.u, g.v),
            (g.x + g.w, g.y, g.u + g.uw, g.v),
            (g.x + g.w, g.y + g.h, g.u + g.uw, g.v + g.vh),
            (g.x, g.y + g.h, g.u, g.v + g.vh),
        ];
        for (x, y, u, v) in corners {
            mesh.vertices.push(Vertex {
                pos: pos2(x, y),
                uv: pos2(u, v),
                color: tint,
            });
        }
        mesh.add_triangle(base, base + 1, base + 2);
        mesh.add_triangle(base, base + 2, base + 3);
    }
    mesh
}

fn glyph_texture(
    shared: &Shared,
    painter: &mut Painter,
    atlas: &[u8],
    w: u32,
    h: u32,
) -> TextureId {
    let size = wgpu::Extent3d {
        width: w.max(1),
        height: h.max(1),
        depth_or_array_layers: 1,
    };
    let texture = shared.device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let rgba: Vec<u8> = atlas.iter().flat_map(|&c| [c, c, c, c]).collect();
    shared.queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &rgba,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(w.max(1) * 4),
            rows_per_image: Some(h.max(1)),
        },
        size,
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    painter
        .renderer
        .register_native_texture(&shared.device, &view, wgpu::FilterMode::Nearest)
}

fn primitives(
    runs: &[Run],
    vw: f32,
    vh: f32,
    tess: &mut Tessellator,
    glyph_tex: Option<TextureId>,
) -> Vec<ClippedPrimitive> {
    let mut out = Vec::new();
    for run in runs {
        let clip = clip_rect(run.clip, vw, vh);
        let mesh = match &run.body {
            Body::Rects(rects) => rect_mesh(tess, rects),
            Body::Glyphs(glyphs) => match glyph_tex {
                Some(tex) => glyph_mesh(tex, glyphs),
                None => continue,
            },
        };
        if mesh.indices.is_empty() {
            continue;
        }
        out.push(ClippedPrimitive {
            clip_rect: clip,
            primitive: Primitive::Mesh(mesh),
        });
    }
    out
}

pub fn present_draw(
    shared: &Shared,
    surface: &wgpu::Surface,
    w: u32,
    h: u32,
    configured: &mut Option<(u32, u32)>,
    clear: [f32; 4],
    runs: &[Run],
    atlas: Option<(&[u8], u32, u32)>,
) -> bool {
    let Some((_config, format)) = configure_surface(shared, surface, w, h, configured) else {
        return false;
    };
    let frame = match surface.get_current_texture() {
        wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
        wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
            *configured = None;
            let Some((_config, _)) = configure_surface(shared, surface, w, h, configured) else {
                return false;
            };
            match surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(f)
                | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
                wgpu::CurrentSurfaceTexture::Timeout
                | wgpu::CurrentSurfaceTexture::Occluded
                | wgpu::CurrentSurfaceTexture::Outdated
                | wgpu::CurrentSurfaceTexture::Lost
                | wgpu::CurrentSurfaceTexture::Validation => return false,
            }
        }
        wgpu::CurrentSurfaceTexture::Timeout
        | wgpu::CurrentSurfaceTexture::Occluded
        | wgpu::CurrentSurfaceTexture::Validation => return false,
    };
    let look = frame
        .texture
        .create_view(&wgpu::TextureViewDescriptor::default());
    let mut enc = shared
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    let screen = ScreenDescriptor {
        size_in_pixels: [w, h],
        pixels_per_point: PIXELS_PER_POINT,
    };
    let clear_color = wgpu::Color {
        r: clear[0] as f64,
        g: clear[1] as f64,
        b: clear[2] as f64,
        a: clear[3] as f64,
    };
    let attachments = [Some(wgpu::RenderPassColorAttachment {
        view: &look,
        depth_slice: None,
        resolve_target: None,
        ops: wgpu::Operations {
            load: wgpu::LoadOp::Clear(clear_color),
            store: wgpu::StoreOp::Store,
        },
    })];
    let pass_desc = wgpu::RenderPassDescriptor {
        label: None,
        color_attachments: &attachments,
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    };
    let mut glyph_tex = None;
    if runs.is_empty() {
        let _ = enc.begin_render_pass(&pass_desc);
    } else {
        with_painter(shared, format, |painter| {
            let has_glyphs = runs
                .iter()
                .any(|r| matches!(&r.body, Body::Glyphs(g) if !g.is_empty()));
            if let (Some((bytes, aw, ah)), true) = (atlas, has_glyphs) {
                glyph_tex = Some(glyph_texture(shared, painter, bytes, aw, ah));
            }
            let mut tess = Tessellator::new(
                PIXELS_PER_POINT,
                TessellationOptions::default(),
                [1, 1],
                Vec::new(),
            );
            let prims = primitives(runs, w as f32, h as f32, &mut tess, glyph_tex);
            painter.renderer.update_buffers(
                &shared.device,
                &shared.queue,
                &mut enc,
                &prims,
                &screen,
            );
            let mut pass = enc.begin_render_pass(&pass_desc).forget_lifetime();
            painter.renderer.render(&mut pass, &prims, &screen);
        });
    }
    shared.queue.submit(Some(enc.finish()));
    shared.queue.present(frame);
    if let Some(id) = glyph_tex {
        with_painter(shared, format, |painter| painter.renderer.free_texture(&id));
    }
    true
}
