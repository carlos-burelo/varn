use std::sync::OnceLock;

use egui::{Color32, Context, FontDefinitions, FontId, RawInput};

pub fn font_definitions() -> FontDefinitions {
    FontDefinitions::default()
}

pub struct ShapedGlyph {
    pub ch: char,
    pub advance: f32,
    pub cluster: u32,
}

pub struct ShapedRun {
    pub glyphs: Vec<ShapedGlyph>,
    pub width: f32,
}

static LAYOUT: OnceLock<Context> = OnceLock::new();

fn layout_context() -> &'static Context {
    LAYOUT.get_or_init(|| {
        let ctx = Context::default();
        ctx.set_fonts(font_definitions());
        let _ = ctx.run_ui(RawInput::default(), |_| {});
        ctx
    })
}

fn check_size(size: f32, op: &str) -> Result<(), String> {
    if size <= 0.0 || !size.is_finite() {
        return Err(format!("text.{op}: size must be positive"));
    }
    Ok(())
}

fn font(size: f32) -> FontId {
    FontId::proportional(size)
}

pub fn fonts() -> Vec<String> {
    let mut out: Vec<String> = layout_context().fonts(|f| {
        f.families()
            .into_iter()
            .map(|family| format!("{family:?}"))
            .collect()
    });
    out.sort();
    out
}

pub fn shape_run(text: &str, size: f32) -> Result<ShapedRun, String> {
    check_size(size, "shape")?;
    let galley = layout_context().fonts_mut(|f| {
        f.layout_no_wrap(text.to_string(), font(size), Color32::WHITE)
    });
    let mut glyphs = Vec::new();
    for row in &galley.rows {
        for g in &row.glyphs {
            glyphs.push(ShapedGlyph {
                ch: g.chr,
                advance: g.advance_width,
                cluster: glyphs.len() as u32,
            });
        }
    }
    let width = glyphs.iter().map(|g| g.advance).sum();
    Ok(ShapedRun { glyphs, width })
}

fn line_metrics(size: f32, op: &str) -> Result<(f32, f32), String> {
    check_size(size, op)?;
    let galley = layout_context().fonts_mut(|f| {
        f.layout_no_wrap("Hg".to_string(), font(size), Color32::WHITE)
    });
    let row = galley
        .rows
        .first()
        .ok_or_else(|| format!("text.{op}: no line"))?;
    let ascent = row.glyphs.first().map_or(0.0, |g| g.font_ascent);
    Ok((ascent, row.rect().height() - ascent))
}

pub fn font_ascent(size: f32) -> Result<f32, String> {
    line_metrics(size, "ascent").map(|(ascent, _)| ascent)
}

pub fn font_descent(size: f32) -> Result<f32, String> {
    line_metrics(size, "descent").map(|(_, descent)| descent)
}

pub fn wrap_lines(text: &str, size: f32, max_width: f32) -> Result<Vec<String>, String> {
    check_size(size, "wrap")?;
    if max_width <= 0.0 || !max_width.is_finite() {
        return Err("text.wrap: max width must be positive".to_string());
    }
    if text.is_empty() {
        return Ok(Vec::new());
    }
    let galley = layout_context().fonts_mut(|f| {
        f.layout(text.to_string(), font(size), Color32::WHITE, max_width)
    });
    Ok(galley
        .rows
        .iter()
        .map(|row| {
            row.glyphs
                .iter()
                .map(|g| g.chr)
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect())
}
