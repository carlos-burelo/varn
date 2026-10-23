use egui::{Context, Event};
use egui_wgpu::{Renderer, RendererOptions, ScreenDescriptor};
use parking_lot::Mutex;
use rustc_hash::FxHashMap;

use super::paint::pass;
use super::quad::{Feedback, Run};
use super::render::{configure_surface, Shared};

const PIXELS_PER_POINT: f32 = 1.0;

struct Backend {
    renderer: Renderer,
}

static PAINTERS: Mutex<Option<FxHashMap<wgpu::TextureFormat, Backend>>> = Mutex::new(None);

fn with_backend<T>(
    shared: &Shared,
    format: wgpu::TextureFormat,
    f: impl FnOnce(&mut Backend) -> T,
) -> T {
    let mut guard = PAINTERS.lock();
    let map = guard.get_or_insert_with(FxHashMap::default);
    let backend = map.entry(format).or_insert_with(|| Backend {
        renderer: Renderer::new(&shared.device, format, RendererOptions::default()),
    });
    f(backend)
}

pub fn present_draw(
    shared: &Shared,
    surface: &wgpu::Surface,
    ctx: &Context,
    events: Vec<Event>,
    time: f64,
    w: u32,
    h: u32,
    configured: &mut Option<(u32, u32)>,
    clear: [f32; 4],
    runs: &[Run],
    warm: Option<egui::TexturesDelta>,
) -> (bool, Feedback) {
    let Some((_config, format)) = configure_surface(shared, surface, w, h, configured) else {
        return (false, Feedback::default());
    };
    let frame = match surface.get_current_texture() {
        wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
        wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
            *configured = None;
            let Some((_config, _)) = configure_surface(shared, surface, w, h, configured) else {
                return (false, Feedback::default());
            };
            match surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(f)
                | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
                wgpu::CurrentSurfaceTexture::Timeout
                | wgpu::CurrentSurfaceTexture::Occluded
                | wgpu::CurrentSurfaceTexture::Outdated
                | wgpu::CurrentSurfaceTexture::Lost
                | wgpu::CurrentSurfaceTexture::Validation => return (false, Feedback::default()),
            }
        }
        wgpu::CurrentSurfaceTexture::Timeout
        | wgpu::CurrentSurfaceTexture::Occluded
        | wgpu::CurrentSurfaceTexture::Validation => return (false, Feedback::default()),
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

    let drawn = pass(ctx, events, time, w, h, runs);

    with_backend(shared, format, |backend| {
        if let Some(warm) = &warm {
            for (id, deltas) in &warm.set {
                for delta in deltas {
                    backend
                        .renderer
                        .update_texture(&shared.device, &shared.queue, *id, delta);
                }
            }
        }
        for (id, deltas) in &drawn.textures.set {
            for delta in deltas {
                backend
                    .renderer
                    .update_texture(&shared.device, &shared.queue, *id, delta);
            }
        }
        backend.renderer.update_buffers(
            &shared.device,
            &shared.queue,
            &mut enc,
            &drawn.prims,
            &screen,
        );
        let mut rpass = enc.begin_render_pass(&pass_desc).forget_lifetime();
        backend.renderer.render(&mut rpass, &drawn.prims, &screen);
    });
    shared.queue.submit(Some(enc.finish()));
    shared.queue.present(frame);
    with_backend(shared, format, |backend| {
        for id in &drawn.textures.free {
            backend.renderer.free_texture(id);
        }
    });
    (true, drawn.feedback)
}
