use egui::Context;

use super::events::RealEvent;
use super::input;
use super::paint::pass;
use super::quad::{Activation, Run};

pub struct Canvas {
    ctx: Context,
    input: Vec<egui::Event>,
}

impl Canvas {
    pub fn new() -> Self {
        let ctx = Context::default();
        Self {
            ctx,
            input: Vec::new(),
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

    pub fn interact(&mut self, w: u32, h: u32, runs: &[Run]) -> Vec<Activation> {
        let events = self.take_input();
        pass(&self.ctx, events, w, h, runs, None).hits
    }
}

impl Default for Canvas {
    fn default() -> Self {
        Self::new()
    }
}
