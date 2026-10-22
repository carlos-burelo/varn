use varn_ui::{Body, Clip, Control, ControlKind, Rect, Run, TextRun};

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
    Control {
        id: u32,
        kind: ControlKind,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        color: i64,
        flag: bool,
        value: f64,
        low: f64,
        high: f64,
        text: String,
        hint: String,
        size: f64,
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
        DrawCmd::Clip { x, y, w, h } => format!("clip {x} {y} {w} {h}"),
        DrawCmd::Unclip => "unclip".to_string(),
        DrawCmd::Control { id, kind, x, y, w, h, color, flag, .. } => {
            format!("control {id} {kind:?} {x} {y} {w} {h} {color} {flag}")
        }
    }
}

pub struct Scene {
    pub clear: [f32; 4],
    pub runs: Vec<Run>,
}

enum Item {
    Rect(Option<Clip>, Rect),
    Text(Option<Clip>, f64, f64, String, f32, [f32; 4]),
    Control(Option<Clip>, Control),
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
            DrawCmd::Control {
                id,
                kind,
                x,
                y,
                w,
                h,
                color,
                flag,
                value,
                low,
                high,
                text,
                hint,
                size,
            } => {
                items.push(Item::Control(
                    current,
                    Control {
                        id: *id,
                        kind: *kind,
                        x: *x as f32,
                        y: *y as f32,
                        w: *w as f32,
                        h: *h as f32,
                        color: varn_ui::argb_to_float(*color),
                        flag: *flag,
                        value: *value as f32,
                        low: *low as f32,
                        high: *high as f32,
                        text: text.clone(),
                        hint: hint.clone(),
                        size: *size as f32,
                    },
                ));
            }
        }
    }
    let mut runs: Vec<Run> = Vec::new();
    for item in &items {
        match item {
            Item::Rect(clip, rect) => push_rect(&mut runs, *clip, rect.clone()),
            Item::Text(clip, x, y, s, size, color) => runs.push(Run {
                clip: *clip,
                body: Body::Text(TextRun {
                    x: *x as f32,
                    y: *y as f32,
                    size: *size,
                    color: *color,
                    text: s.clone(),
                }),
            }),
            Item::Control(clip, control) => runs.push(Run {
                clip: *clip,
                body: Body::Control(control.clone()),
            }),
        }
    }
    Ok(Scene { clear, runs })
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

