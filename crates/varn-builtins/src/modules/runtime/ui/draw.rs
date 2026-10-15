use varn_ui::{Glyph, Rect};

#[derive(Clone)]
pub enum DrawCmd {
    Clear(i64),
    Rect {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        color: i64,
        radius: f64,
    },
    Text {
        x: f64,
        y: f64,
        s: String,
        size: f64,
        color: i64,
    },
}

pub fn format(cmd: &DrawCmd) -> String {
    match cmd {
        DrawCmd::Clear(c) => format!("clear {c}"),
        DrawCmd::Rect {
            x,
            y,
            w,
            h,
            color,
            radius,
        } => format!("rect {x} {y} {w} {h} {color} {radius}"),
        DrawCmd::Text {
            x,
            y,
            s,
            size,
            color,
        } => format!("text {x} {y} {size} {color} {s}"),
    }
}

pub struct Scene {
    pub clear: [f32; 4],
    pub rects: Vec<Rect>,
    pub glyphs: Vec<Glyph>,
    pub atlas: Option<(Vec<u8>, u32, u32)>,
}

pub fn assemble(cmds: &[DrawCmd]) -> Result<Scene, String> {
    let mut clear = varn_ui::argb_to_float(0xFF000000u32 as i64);
    let mut rects = Vec::new();
    let mut texts: Vec<(f64, f64, &str, f64, [f32; 4])> = Vec::new();
    for cmd in cmds {
        match cmd {
            DrawCmd::Clear(c) => clear = varn_ui::argb_to_float(*c),
            DrawCmd::Rect {
                x,
                y,
                w,
                h,
                color,
                radius,
            } => {
                if *w > 0.0 && *h > 0.0 {
                    let r = radius.clamp(0.0, w.min(*h) / 2.0);
                    rects.push(Rect {
                        x: *x as f32,
                        y: *y as f32,
                        w: *w as f32,
                        h: *h as f32,
                        color: varn_ui::argb_to_float(*color),
                        radius: r as f32,
                    });
                }
            }
            DrawCmd::Text {
                x,
                y,
                s,
                size,
                color,
            } => {
                if *size > 0.0 && !s.is_empty() {
                    texts.push((*x, *y, s, *size, varn_ui::argb_to_float(*color)));
                }
            }
        }
    }
    let mut glyphs = Vec::new();
    let mut atlas = None;
    if !texts.is_empty() {
        let mut items: Vec<(u32, u32, Vec<u8>)> = Vec::new();
        let mut quads: Vec<(f32, f32, f32, f32, usize, [f32; 4])> = Vec::new();
        for (x, y, s, size, color) in &texts {
            let ascent = varn_text::font_ascent(*size as f32)?;
            let baseline = *y as f32 + ascent;
            let run = varn_text::shape_run(s, *size as f32)?;
            let mut pen = *x as f32;
            for g in &run.glyphs {
                let r = varn_text::raster_glyph(g.id, g.advance, *size as f32)?;
                let gx = pen + g.offset_x + r.bx as f32;
                let gy = baseline - r.by as f32;
                if r.w > 0 && r.h > 0 {
                    quads.push((gx, gy, r.w as f32, r.h as f32, items.len(), *color));
                    items.push((r.w, r.h, r.bitmap));
                }
                pen += g.advance;
            }
        }
        if !items.is_empty() {
            let packed = varn_text::pack_bitmaps(&items, 1024);
            let aw = packed.w as f32;
            let ah = packed.h.max(1) as f32;
            for (gx, gy, w, h, idx, color) in quads {
                let (px, py) = packed.placements[idx];
                glyphs.push(Glyph {
                    x: gx,
                    y: gy,
                    w,
                    h,
                    u: px as f32 / aw,
                    v: py as f32 / ah,
                    uw: w / aw,
                    vh: h / ah,
                    color,
                });
            }
            atlas = Some((packed.data, packed.w, packed.h.max(1)));
        }
    }
    Ok(Scene {
        clear,
        rects,
        glyphs,
        atlas,
    })
}
