use egui::{
    emath::{pos2, vec2},
    epaint::{ClippedPrimitive, TextureId, Vertex},
    Button, Checkbox, Color32, Context, Event, Mesh, Rect as ERect, RawInput, ScrollArea, Sense,
    scroll_area::ScrollSource, Shape, Slider, TexturesDelta, Ui, UiBuilder,
};

use super::quad::{Activation, Body, Clip, Control, ControlKind, Glyph, Rect, Run};

const MAX_CORNER: f32 = 255.0;

pub struct Pass {
    pub prims: Vec<ClippedPrimitive>,
    pub textures: TexturesDelta,
    pub hits: Vec<Activation>,
}

pub fn pass(
    ctx: &Context,
    events: Vec<Event>,
    w: u32,
    h: u32,
    runs: &[Run],
    glyph_tex: Option<TextureId>,
) -> Pass {
    let input = RawInput {
        screen_rect: Some(ERect::from_min_size(
            pos2(0.0, 0.0),
            vec2(w as f32, h as f32),
        )),
        events,
        ..Default::default()
    };
    let mut hits = Vec::new();
    let output = ctx.run_ui(input, |ui| {
        paint_runs(ui, runs, w as f32, h as f32, glyph_tex, &mut hits);
    });
    let prims = ctx.tessellate(output.shapes, output.pixels_per_point);
    Pass {
        prims,
        textures: output.textures_delta,
        hits,
    }
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

fn paint_rects(ui: &Ui, rects: &[Rect]) {
    let painter = ui.painter();
    for r in rects {
        painter.rect_filled(
            ERect::from_min_size(pos2(r.x, r.y), vec2(r.w, r.h)),
            corner(r.radius),
            color(r.color),
        );
    }
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

fn paint_switch(ui: &mut Ui, rect: ERect, c: &Control) -> Option<Activation> {
    let response = ui.interact(rect, ui.scope_id().with("switch"), Sense::click());
    let track = if c.flag {
        color(c.color)
    } else {
        Color32::from_gray(96)
    };
    let knob = rect.height() - 6.0;
    let knob_x = if c.flag {
        rect.right() - knob - 3.0
    } else {
        rect.left() + 3.0
    };
    let painter = ui.painter();
    painter.rect_filled(rect, corner(rect.height() / 2.0), track);
    painter.rect_filled(
        ERect::from_min_size(pos2(knob_x, rect.top() + 3.0), vec2(knob, knob)),
        corner(knob / 2.0),
        Color32::WHITE,
    );
    response.clicked().then_some(Activation { id: c.id, value: 0.0 })
}

fn paint_scroll(ui: &mut Ui, rect: ERect, c: &Control) -> Option<Activation> {
    let extent = c.high;
    let out = ui
        .scope_builder(UiBuilder::new().max_rect(rect), |ui| {
            ScrollArea::vertical()
                .id_salt(ui.scope_id().with("scroll"))
                .auto_shrink([false, false])
                .animated(false)
                .scroll_source(ScrollSource {
                    mouse_wheel: false,
                    ..ScrollSource::ALL
                })
                .vertical_scroll_offset(c.value)
                .show_viewport(ui, |ui, _viewport| {
                    ui.allocate_space(vec2(c.w, extent));
                })
        })
        .inner;
    let offset = out.state.clamped_offset().y;
    ((offset - c.value).abs() > 0.5).then_some(Activation {
        id: c.id,
        value: offset,
    })
}

fn paint_control(ui: &mut Ui, c: &Control, hits: &mut Vec<Activation>) {
    let rect = ERect::from_min_size(pos2(c.x, c.y), vec2(c.w, c.h));
    let outcome = ui
        .push_id(c.id, |ui| match c.kind {
            ControlKind::Button => ui
                .put(
                    rect,
                    Button::new("")
                        .fill(color(c.color))
                        .min_size(vec2(c.w, c.h)),
                )
                .clicked()
                .then_some(Activation { id: c.id, value: 0.0 }),
            ControlKind::Checkbox => {
                let mut on = c.flag;
                ui.put(rect, Checkbox::without_text(&mut on))
                    .clicked()
                    .then_some(Activation { id: c.id, value: 0.0 })
            }
            ControlKind::Switch => paint_switch(ui, rect, c),
            ControlKind::Slider => {
                let mut v = c.value;
                ui.scope(|ui| {
                    ui.spacing_mut().slider_width = c.w;
                    ui.put(rect, Slider::new(&mut v, c.low..=c.high).show_value(false))
                        .changed()
                        .then_some(Activation { id: c.id, value: v })
                })
                .inner
            }
            ControlKind::Scroll => paint_scroll(ui, rect, c),
            ControlKind::Surface => ui
                .interact(rect, ui.scope_id().with("surface"), Sense::click())
                .clicked()
                .then_some(Activation { id: c.id, value: 0.0 }),
        })
        .inner;
    hits.extend(outcome);
}

fn paint_runs(
    ui: &mut Ui,
    runs: &[Run],
    vw: f32,
    vh: f32,
    glyph_tex: Option<TextureId>,
    hits: &mut Vec<Activation>,
) {
    for run in runs {
        let clip = clip_rect(run.clip, vw, vh);
        match &run.body {
            Body::Rects(rects) => {
                ui.scope(|ui| {
                    ui.set_clip_rect(clip);
                    paint_rects(ui, rects);
                });
            }
            Body::Glyphs(glyphs) => {
                if let Some(tex) = glyph_tex {
                    ui.painter()
                        .with_clip_rect(clip)
                        .add(Shape::mesh(glyph_mesh(tex, glyphs)));
                }
            }
            Body::Control(c) => {
                ui.scope(|ui| {
                    ui.set_clip_rect(clip);
                    paint_control(ui, c, hits);
                });
            }
        }
    }
}
