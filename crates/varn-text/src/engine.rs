use parking_lot::Mutex;
use parley::{FontContext, FontData, Layout, LayoutContext, StyleProperty};
use std::sync::OnceLock;

pub(crate) struct Engine {
    pub(crate) fcx: FontContext,
    pub(crate) lcx: LayoutContext<()>,
    pub(crate) fonts: Vec<FontData>,
}

static ENGINE: OnceLock<Mutex<Engine>> = OnceLock::new();

pub(crate) fn engine() -> &'static Mutex<Engine> {
    ENGINE.get_or_init(|| {
        Mutex::new(Engine {
            fcx: FontContext::new(),
            lcx: LayoutContext::new(),
            fonts: Vec::new(),
        })
    })
}

impl Engine {
    pub(crate) fn font_index(&mut self, font: &FontData) -> u32 {
        if let Some(at) = self
            .fonts
            .iter()
            .position(|f| f.data.id() == font.data.id() && f.index == font.index)
        {
            return at as u32;
        }
        self.fonts.push(font.clone());
        (self.fonts.len() - 1) as u32
    }
}

pub(crate) fn check_size(size: f32, op: &str) -> Result<(), String> {
    if size <= 0.0 || !size.is_finite() {
        return Err(format!("text.{op}: size must be positive"));
    }
    Ok(())
}

pub(crate) fn lay_out(text: &str, size: f32, max_width: Option<f32>) -> Result<Layout<()>, String> {
    check_size(size, "layout")?;
    let mut guard = engine().lock();
    let eng = &mut *guard;
    let mut builder = eng.lcx.ranged_builder(&mut eng.fcx, text, 1.0, true);
    builder.push_default(StyleProperty::FontSize(size));
    let mut layout: Layout<()> = builder.build(text);
    layout.break_all_lines(max_width);
    Ok(layout)
}
