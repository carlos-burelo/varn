use super::engine::{check_size, engine, lay_out};
use parley::PositionedLayoutItem;

pub struct ShapedGlyph {
    pub id: u16,
    pub font: u32,
    pub advance: f32,
    pub x: f32,
    pub cluster: u32,
}

pub struct ShapedRun {
    pub glyphs: Vec<ShapedGlyph>,
    pub width: f32,
}

pub fn fonts() -> Vec<String> {
    let mut guard = engine().lock();
    let mut out: Vec<String> = guard
        .fcx
        .collection
        .family_names()
        .map(str::to_owned)
        .collect();
    out.sort();
    out.dedup();
    out
}

pub fn shape_run(text: &str, size: f32) -> Result<ShapedRun, String> {
    let layout = lay_out(text, size, None)?;
    let mut guard = engine().lock();
    let mut glyphs = Vec::new();
    for line in layout.lines() {
        for item in line.items() {
            if let PositionedLayoutItem::GlyphRun(run) = item {
                let font = guard.font_index(run.run().font());
                let mut pen = run.offset();
                for cluster in run.run().clusters() {
                    let at = cluster.text_range().start as u32;
                    for g in cluster.glyphs() {
                        glyphs.push(ShapedGlyph {
                            id: g.id as u16,
                            font,
                            advance: g.advance,
                            x: pen + g.x,
                            cluster: at,
                        });
                        pen += g.advance;
                    }
                }
            }
        }
    }
    let width = glyphs.iter().map(|g| g.advance).sum();
    Ok(ShapedRun { glyphs, width })
}

fn line_metrics(size: f32, op: &str) -> Result<(f32, f32), String> {
    check_size(size, op)?;
    let layout = lay_out("Hg", size, None)?;
    let line = layout
        .lines()
        .next()
        .ok_or_else(|| format!("text.{op}: no line"))?;
    let metrics = line.metrics();
    Ok((metrics.baseline, metrics.line_height - metrics.baseline))
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
    let layout = lay_out(text, size, Some(max_width))?;
    layout
        .lines()
        .map(|line| {
            text.get(line.text_range())
                .map(|s| s.trim_end().to_string())
                .ok_or_else(|| "text.wrap: line outside the text".to_string())
        })
        .collect()
}
