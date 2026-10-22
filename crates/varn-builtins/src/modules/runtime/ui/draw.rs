use varn_ui::{Body, Clip, Glyph, Rect, Run};

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
    Clip {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    },
    Unclip,
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
        DrawCmd::Clip { x, y, w, h } => format!("clip {x} {y} {w} {h}"),
        DrawCmd::Unclip => "unclip".to_string(),
    }
}

pub struct Scene {
    pub clear: [f32; 4],
    pub runs: Vec<Run>,
    pub atlas: Option<(Vec<u8>, u32, u32)>,
}

enum Item {
    Rect(Option<Clip>, Rect),
    Text(Option<Clip>, f64, f64, String, f32, [f32; 4]),
}

fn intersect(outer: Option<Clip>, inner: Clip) -> Clip {
    let Some(o) = outer else {
        return inner;
    };
    let x0 = o.x.max(inner.x);
    let y0 = o.y.max(inner.y);
    let x1 = (o.x + o.w).min(inner.x + inner.w);
    let y1 = (o.y + o.h).min(inner.y + inner.h);
    Clip {
        x: x0,
        y: y0,
        w: (x1 - x0).max(0.0),
        h: (y1 - y0).max(0.0),
    }
}

pub fn assemble(cmds: &[DrawCmd]) -> Result<Scene, String> {
    let mut clear = varn_ui::argb_to_float(0xFF000000u32 as i64);
    let mut items: Vec<Item> = Vec::new();
    let mut current: Option<Clip> = None;
    let mut saved: Vec<Option<Clip>> = Vec::new();
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
                    items.push(Item::Rect(
                        current,
                        Rect {
                            x: *x as f32,
                            y: *y as f32,
                            w: *w as f32,
                            h: *h as f32,
                            color: varn_ui::argb_to_float(*color),
                            radius: r as f32,
                        },
                    ));
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
                    items.push(Item::Text(
                        current,
                        *x,
                        *y,
                        s.clone(),
                        *size as f32,
                        varn_ui::argb_to_float(*color),
                    ));
                }
            }
            DrawCmd::Clip { x, y, w, h } => {
                saved.push(current);
                current = Some(intersect(
                    current,
                    Clip {
                        x: *x as f32,
                        y: *y as f32,
                        w: *w as f32,
                        h: *h as f32,
                    },
                ));
            }
            DrawCmd::Unclip => {
                current = saved.pop().unwrap_or(None);
            }
        }
    }
    let (glyph_sets, atlas) = shape(&items)?;
    let mut runs: Vec<Run> = Vec::new();
    let mut text_ordinal = 0;
    for item in &items {
        match item {
            Item::Rect(clip, rect) => push_rect(&mut runs, *clip, rect.clone()),
            Item::Text(clip, ..) => {
                for glyph in &glyph_sets[text_ordinal] {
                    push_glyph(&mut runs, *clip, glyph.clone());
                }
                text_ordinal += 1;
            }
        }
    }
    Ok(Scene {
        clear,
        runs,
        atlas,
    })
}

fn push_rect(runs: &mut Vec<Run>, clip: Option<Clip>, rect: Rect) {
    if let Some(Run {
        clip: last,
        body: Body::Rects(rects),
    }) = runs.last_mut()
        && *last == clip
    {
        rects.push(rect);
        return;
    }
    runs.push(Run {
        clip,
        body: Body::Rects(vec![rect]),
    });
}

fn push_glyph(runs: &mut Vec<Run>, clip: Option<Clip>, glyph: Glyph) {
    if let Some(Run {
        clip: last,
        body: Body::Glyphs(glyphs),
    }) = runs.last_mut()
        && *last == clip
    {
        glyphs.push(glyph);
        return;
    }
    runs.push(Run {
        clip,
        body: Body::Glyphs(vec![glyph]),
    });
}

type Quad = (f32, f32, f32, f32, usize, [f32; 4]);

fn shape(items: &[Item]) -> Result<(Vec<Vec<Glyph>>, Option<(Vec<u8>, u32, u32)>), String> {
    let texts: Vec<(&f64, &f64, &String, f32, [f32; 4])> = items
        .iter()
        .filter_map(|item| match item {
            Item::Text(_, x, y, s, size, color) => Some((x, y, s, *size, *color)),
            Item::Rect(..) => None,
        })
        .collect();
    let mut quad_sets: Vec<Vec<Quad>> = Vec::with_capacity(texts.len());
    let mut bitmaps: Vec<(u32, u32, Vec<u8>)> = Vec::new();
    for (x, y, s, size, color) in &texts {
        let ascent = varn_text::font_ascent(*size)?;
        let baseline = **y as f32 + ascent;
        let run = varn_text::shape_run(s, *size)?;
        let origin = **x as f32;
        let mut quads = Vec::new();
        for g in &run.glyphs {
            let at = origin + g.x;
            let whole = at.floor();
            let steps = f32::from(varn_text::SUBPIXEL_STEPS);
            let mut sub = ((at - whole) * steps).round();
            let mut whole = whole;
            if sub >= steps {
                sub = 0.0;
                whole += 1.0;
            }
            let r = varn_text::raster_glyph(g.font, g.id, *size, sub as u8)?;
            let gx = whole + r.bx as f32;
            let gy = (baseline - r.by as f32).round();
            if r.w > 0 && r.h > 0 {
                quads.push((gx, gy, r.w as f32, r.h as f32, bitmaps.len(), *color));
                bitmaps.push((r.w, r.h, r.bitmap));
            }
        }
        quad_sets.push(quads);
    }
    if bitmaps.is_empty() {
        return Ok((vec![Vec::new(); quad_sets.len()], None));
    }
    let packed = varn_text::pack_bitmaps(&bitmaps, 1024);
    let aw = packed.w as f32;
    let ah = packed.h.max(1) as f32;
    let glyph_sets = quad_sets
        .into_iter()
        .map(|quads| {
            quads
                .into_iter()
                .map(|(gx, gy, w, h, idx, color)| {
                    let (px, py) = packed.placements[idx];
                    Glyph {
                        x: gx,
                        y: gy,
                        w,
                        h,
                        u: px as f32 / aw,
                        v: py as f32 / ah,
                        uw: w / aw,
                        vh: h / ah,
                        color,
                    }
                })
                .collect()
        })
        .collect();
    Ok((
        glyph_sets,
        Some((packed.data, packed.w, packed.h.max(1))),
    ))
}
