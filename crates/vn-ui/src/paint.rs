use egui::{
    emath::{pos2, vec2},
    epaint::ClippedPrimitive,
    scroll_area::State as ScrollState,
    text::{CCursor, CCursorRange},
    Align2, Button, Color32, Context, Event, FontId, Key, RawInput, Rect as ERect, ScrollArea,
    Sense, Slider, TexturesDelta, Ui, UiBuilder,
};

use super::quad::{Activation, Body, Clip, Control, ControlKind, Feedback, Rect, Run, TextChange};

const MAX_CORNER: f32 = 255.0;
const FIELD_PAD: f32 = 12.0;

const FIELD_FOCUS_GAINED: f32 = 1.0;
const FIELD_BLURRED: f32 = 0.0;
const FIELD_SUBMITTED: f32 = 2.0;

#[derive(Clone)]
struct FieldText {
    model: String,
    text: String,
}

pub struct Pass {
    pub prims: Vec<ClippedPrimitive>,
    pub textures: TexturesDelta,
    pub feedback: Feedback,
}

pub fn pass(ctx: &Context, events: Vec<Event>, time: f64, w: u32, h: u32, runs: &[Run]) -> Pass {
    let input = RawInput {
        screen_rect: Some(ERect::from_min_size(
            pos2(0.0, 0.0),
            vec2(w as f32, h as f32),
        )),
        events,
        time: Some(time),
        ..Default::default()
    };
    let mut feedback = Feedback::default();
    let output = ctx.run_ui(input, |ui| {
        paint_runs(ui, runs, w as f32, h as f32, &mut feedback);
    });
    let prims = ctx.tessellate(output.shapes, output.pixels_per_point);
    Pass {
        prims,
        textures: output.textures_delta,
        feedback,
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
    response.clicked().then_some(Activation {
        id: c.id,
        value: 0.0,
    })
}

fn paint_scroll(ui: &mut Ui, rect: ERect, c: &Control) -> Option<Activation> {
    let extent = c.high;
    let salt = ("vn-scroll", c.id);
    let memory = ui.make_persistent_id(("vn-scroll-sent", c.id));
    let mut sent = ui.data(|d| d.get_temp::<f32>(memory)).unwrap_or(c.value);
    if (sent - c.value).abs() > 0.5 {
        sent = c.value;
    }
    let forced = sent;
    let out = ui
        .scope_builder(UiBuilder::new().max_rect(rect), |ui| {
            let mut area = ScrollArea::vertical()
                .id_salt(salt)
                .auto_shrink([false, false]);
            if (ScrollState::load(ui.ctx(), ui.make_persistent_id(salt))
                .map_or(0.0, |s| s.clamped_offset().y)
                - forced)
                .abs()
                > 0.5
            {
                area = area.vertical_scroll_offset(forced);
            }
            area.show_viewport(ui, |ui, _viewport| {
                ui.allocate_space(vec2(c.w, extent));
            })
        })
        .inner;
    let offset = out.state.clamped_offset().y;
    if (offset - sent).abs() > 0.5 {
        ui.data_mut(|d| d.insert_temp(memory, offset));
        Some(Activation {
            id: c.id,
            value: offset,
        })
    } else {
        ui.data_mut(|d| d.insert_temp(memory, sent));
        None
    }
}

fn paint_field(ui: &mut Ui, rect: ERect, c: &Control, feedback: &mut Feedback) {
    let multiline = c.kind == ControlKind::Area;
    let inner = ERect::from_min_max(
        pos2(rect.left() + FIELD_PAD, rect.top()),
        pos2(rect.right() - FIELD_PAD, rect.bottom()),
    );
    let memory = ui.make_persistent_id(("vn-field-text", c.id));
    let mut state = ui
        .data(|d| d.get_temp::<FieldText>(memory))
        .unwrap_or_else(|| FieldText {
            model: c.text.clone(),
            text: c.text.clone(),
        });
    let edit_id = ui.make_persistent_id(("vn-field", c.id));
    if state.model != c.text {
        state.model = c.text.clone();
        state.text = c.text.clone();
        let mut edit_state =
            egui::text_edit::TextEditState::load(ui.ctx(), edit_id).unwrap_or_default();
        let end = state.text.chars().count();
        edit_state
            .cursor
            .set_char_range(Some(CCursorRange::one(CCursor::new(end))));
        edit_state.store(ui.ctx(), edit_id);
    }
    let edit = if multiline {
        egui::TextEdit::multiline(&mut state.text)
    } else {
        egui::TextEdit::singleline(&mut state.text)
    };
    let enter = !multiline && ui.input(|i| i.key_pressed(Key::Enter));
    let response = ui.put(
        inner,
        edit.id(edit_id)
            .font(FontId::proportional(c.size))
            .frame(egui::Frame::NONE)
            .text_color(color(c.color))
            .hint_text(c.hint.as_str())
            .min_size(inner.size()),
    );

    if !c.flag && response.has_focus() && !response.gained_focus() {
        response.surrender_focus();
    }
    if c.flag && !response.has_focus() {
        response.request_focus();
    }
    if response.gained_focus() {
        feedback.activations.push(Activation {
            id: c.id,
            value: FIELD_FOCUS_GAINED,
        });
    }
    if response.changed() {
        state.model = state.text.clone();
        feedback.texts.push(TextChange {
            id: c.id,
            text: state.text.clone(),
        });
    }
    ui.data_mut(|d| d.insert_temp(memory, state));
    if enter && (response.has_focus() || response.lost_focus()) {
        feedback.activations.push(Activation {
            id: c.id,
            value: FIELD_SUBMITTED,
        });
    }
    if response.lost_focus() {
        feedback.activations.push(Activation {
            id: c.id,
            value: FIELD_BLURRED,
        });
    }
}

fn paint_control(ui: &mut Ui, c: &Control, feedback: &mut Feedback) {
    let rect = ERect::from_min_size(pos2(c.x, c.y), vec2(c.w, c.h));
    if matches!(c.kind, ControlKind::Field | ControlKind::Area) {
        ui.push_id(c.id, |ui| paint_field(ui, rect, c, feedback));
        return;
    }
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
                .then_some(Activation {
                    id: c.id,
                    value: 0.0,
                }),
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
                .then_some(Activation {
                    id: c.id,
                    value: 0.0,
                }),
            ControlKind::Field | ControlKind::Area => None,
        })
        .inner;
    feedback.activations.extend(outcome);
}

fn paint_runs(ui: &mut Ui, runs: &[Run], vw: f32, vh: f32, feedback: &mut Feedback) {
    for run in runs {
        let clip = clip_rect(run.clip, vw, vh);
        match &run.body {
            Body::Rects(rects) => {
                ui.scope(|ui| {
                    ui.set_clip_rect(clip);
                    paint_rects(ui, rects);
                });
            }
            Body::Text(t) => {
                ui.painter().with_clip_rect(clip).text(
                    pos2(t.x, t.y),
                    Align2::LEFT_TOP,
                    &t.text,
                    FontId::proportional(t.size),
                    color(t.color),
                );
            }
            Body::Control(c) => {
                ui.scope(|ui| {
                    ui.set_clip_rect(clip);
                    paint_control(ui, c, feedback);
                });
            }
        }
    }
}
