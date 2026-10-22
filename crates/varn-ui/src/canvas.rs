use egui::Context;

use super::events::RealEvent;
use super::input;

pub struct Canvas {
    ctx: Context,
    input: Vec<egui::Event>,
}

impl Canvas {
    pub fn new() -> Self {
        Self {
            ctx: Context::default(),
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
}

impl Default for Canvas {
    fn default() -> Self {
        Self::new()
    }
}
