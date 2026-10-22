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
}

impl Canvas {
    pub fn new() -> Self {
        Self {
            ctx: Context::default(),
            input: Vec::new(),
            frames: 0,
        }
    }

    pub fn feed(&mut self, e: &RealEvent) {
        input::push(e, &mut self.input);
    }

    pub fn context(&self) -> &Context {
        &self.ctx
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
