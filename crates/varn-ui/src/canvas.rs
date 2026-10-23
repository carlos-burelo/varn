use std::sync::OnceLock;
use std::time::Instant;

use egui::Context;

use super::events::RealEvent;
use super::input;
use super::paint::pass;
use super::quad::{Feedback, Run};

const HEADLESS_FRAME_SECS: f64 = 1.0 / 60.0;

static START: OnceLock<Instant> = OnceLock::new();

pub fn real_time() -> f64 {
    START.get_or_init(Instant::now).elapsed().as_secs_f64()
}

pub struct Canvas {
    ctx: Context,
    input: Vec<egui::Event>,
    frames: u64,
    warmup: Option<egui::TexturesDelta>,
}

impl Canvas {
    pub fn new() -> Self {
        let ctx = Context::default();
        ctx.set_fonts(varn_text::font_definitions());
        let warm = ctx.run_ui(egui::RawInput::default(), |_| {});
        Self {
            ctx,
            input: Vec::new(),
            frames: 0,
            warmup: Some(warm.textures_delta),
        }
    }

    pub fn text_width(&self, text: &str, size: f32) -> f32 {
        let galley = self.ctx.fonts_mut(|f| {
            f.layout_no_wrap(
                text.to_string(),
                egui::FontId::proportional(size),
                egui::Color32::WHITE,
            )
        });
        galley
            .rows
            .iter()
            .flat_map(|row| row.glyphs.iter())
            .map(|g| g.advance_width)
            .sum()
    }

    pub fn feed(&mut self, e: &RealEvent) {
        input::push(e, &mut self.input);
    }

    pub fn context(&self) -> &Context {
        &self.ctx
    }

    pub fn set_dark(&mut self, dark: bool) {
        self.ctx.set_visuals(if dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        });
    }

    pub fn take_warmup(&mut self) -> Option<egui::TexturesDelta> {
        self.warmup.take()
    }

    pub fn take_input(&mut self) -> Vec<egui::Event> {
        std::mem::take(&mut self.input)
    }

    pub fn interact(&mut self, w: u32, h: u32, runs: &[Run]) -> Feedback {
        let events = self.take_input();
        self.frames += 1;
        let time = self.frames as f64 * HEADLESS_FRAME_SECS;
        pass(&self.ctx, events, time, w, h, runs).feedback
    }
}

impl Default for Canvas {
    fn default() -> Self {
        Self::new()
    }
}
