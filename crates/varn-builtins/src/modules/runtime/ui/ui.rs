use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::OnceLock;
use varn_macros::varn_contract;
use varn_types::{NativeCtx, VmValue};

#[path = "draw.rs"]
mod ui_draw;
use ui_draw::DrawCmd;

pub struct UiRuntime;

varn_macros::varn_enum! {
    module: MODULE_ID,
    contract: "src/modules/runtime/ui/ui_runtime.vn",
    pub enum UiEventKind;
}

varn_macros::varn_enum! {
    module: MODULE_ID,
    contract: "src/modules/runtime/ui/ui_runtime.vn",
    pub enum UiInjectKind;
}

varn_macros::varn_enum! {
    module: MODULE_ID,
    contract: "src/modules/runtime/ui/ui_runtime.vn",
    pub enum UiControlKind;
}

struct Window {
    alive: bool,
    closed: bool,
    w: i64,
    h: i64,
    events: VecDeque<varn_ui::RealEvent>,
    commands: Vec<DrawCmd>,
    real: Option<u64>,
    canvas: varn_ui::Canvas,
    activated: Vec<varn_ui::Activation>,
    typed: Vec<varn_ui::TextChange>,
}

static WINDOWS: OnceLock<Mutex<Vec<Option<Window>>>> = OnceLock::new();

fn windows() -> &'static Mutex<Vec<Option<Window>>> {
    WINDOWS.get_or_init(|| Mutex::new(Vec::new()))
}

fn with_win<T>(id: i64, f: impl FnOnce(&mut Window) -> Result<T, String>) -> Result<T, String> {
    if id < 0 {
        return Err("ui: bad window handle".to_string());
    }
    let mut all = windows().lock();
    let w = all
        .get_mut(id as usize)
        .and_then(|s| s.as_mut())
        .filter(|w| w.alive)
        .ok_or_else(|| "ui: destroyed window".to_string())?;
    f(w)
}

fn real_poll(id: u64) -> Vec<varn_ui::RealEvent> {
    varn_ui::take_events(id).unwrap_or_default()
}

fn ui_event(ctx: &mut dyn NativeCtx, e: &varn_ui::RealEvent) -> Result<VmValue, String> {
    let (kind, key, pressed, x, y, button, w, h) = match *e {
        varn_ui::RealEvent::Key { key, pressed } => (UiEventKind::Key, key, pressed, 0, 0, 0, 0, 0),
        varn_ui::RealEvent::Mouse { x, y, button, pressed } => (UiEventKind::Mouse, 0, pressed, x, y, button, 0, 0),
        varn_ui::RealEvent::Resize { w, h } => (UiEventKind::Resize, 0, false, 0, 0, 0, w, h),
        varn_ui::RealEvent::Wheel { dy } => (UiEventKind::Wheel, 0, false, 0, dy, 0, 0, 0),
        varn_ui::RealEvent::Close => (UiEventKind::Close, 0, false, 0, 0, 0, 0, 0),
    };
    let kind = kind.to_value(ctx)?;
    Ok(UiEvent { kind, key, pressed, x, y, button, w, h }.alloc(ctx))
}

varn_macros::varn_record! {
    contract: "src/modules/runtime/ui/ui_runtime.vn",
    interface: "TypedText",
    struct TypedText;
}

varn_macros::varn_record! {
    contract: "src/modules/runtime/ui/ui_runtime.vn",
    interface: "DrawnItem",
    struct DrawnItem;
}

varn_macros::varn_record! {
    contract: "src/modules/runtime/ui/ui_runtime.vn",
    interface: "UiEvent",
    struct UiEvent;
}


varn_contract! {
    module: "runtime:ui",
    contract: "src/modules/runtime/ui/ui_runtime.vn",
    impl UiRuntime {
        fn open(_ctx: &mut dyn NativeCtx, title: &str, w: i64, h: i64, headless: bool) -> Result<i64, String> {
            if w <= 0 || h <= 0 {
                return Err(format!("ui.open: bad size ({w}x{h})"));
            }
            let mut all = windows().lock();
            let real = if !headless && w <= u32::MAX as i64 && h <= u32::MAX as i64 {
                drop(all);
                let r = varn_ui::try_open(title, w as u32, h as u32);
                all = windows().lock();
                r
            } else {
                None
            };
            let id = all.len() as i64;
            all.push(Some(Window {
                alive: true,
                closed: false,
                w,
                h,
                events: VecDeque::new(),
                commands: Vec::new(),
                real,
                canvas: varn_ui::Canvas::new(),
                activated: Vec::new(),
                typed: Vec::new(),
            }));
            Ok(id)
        }
        fn close(_ctx: &mut dyn NativeCtx, id: i64) -> Result<(), String> {
            let real = with_win(id, |w| {
                w.alive = false;
                Ok(w.real)
            })?;
            if let Some(h) = real {
                let _ = varn_ui::close(h);
            }
            Ok(())
        }
        fn shouldClose(_ctx: &mut dyn NativeCtx, id: i64) -> Result<bool, String> {
            with_win(id, |w| Ok(w.closed))
        }
        fn size(_ctx: &mut dyn NativeCtx, id: i64) -> Result<Vec<VmValue>, String> {
            with_win(id, |w| {
                if let Some(h) = w.real
                    && let Ok((rw, rh)) = varn_ui::size(h)
                {
                    w.w = rw as i64;
                    w.h = rh as i64;
                }
                Ok(vec![VmValue::from_int(w.w), VmValue::from_int(w.h)])
            })
        }
        fn inject(ctx: &mut dyn NativeCtx, id: i64, kind: VmValue, a: i64, b: i64, c: i64) -> Result<(), String> {
            let inject = UiInjectKind::from_value(ctx, kind)
                .ok_or_else(|| "ui.inject: kind is not a UiInjectKind".to_string())?;
            with_win(id, |w| {
                let e = match inject {
                    UiInjectKind::Key => varn_ui::RealEvent::Key { key: a, pressed: b != 0 },
                    UiInjectKind::Mouse => varn_ui::RealEvent::Mouse { x: a, y: b, button: c, pressed: false },
                    UiInjectKind::MouseDown => varn_ui::RealEvent::Mouse { x: a, y: b, button: c, pressed: true },
                    UiInjectKind::MouseUp => varn_ui::RealEvent::Mouse { x: a, y: b, button: c, pressed: false },
                    UiInjectKind::Resize => {
                        w.w = a;
                        w.h = b;
                        varn_ui::RealEvent::Resize { w: a, h: b }
                    }
                    UiInjectKind::Wheel => varn_ui::RealEvent::Wheel { dy: a },
                    UiInjectKind::Close => {
                        w.closed = true;
                        varn_ui::RealEvent::Close
                    }
                };
                w.events.push_back(e);
                Ok(())
            })
        }
        fn poll(ctx: &mut dyn NativeCtx, id: i64) -> Result<Vec<VmValue>, String> {
            let real = with_win(id, |w| Ok(w.real))?;
            if let Some(h) = real {
                let _ = varn_ui::pump(h);
                for e in real_poll(h) {
                    with_win(id, |w| {
                        match e {
                            varn_ui::RealEvent::Close => w.closed = true,
                            varn_ui::RealEvent::Resize { w: rw, h: rh } => {
                                w.w = rw;
                                w.h = rh;
                            }
                            varn_ui::RealEvent::Key { .. }
                            | varn_ui::RealEvent::Mouse { .. }
                            | varn_ui::RealEvent::Wheel { .. } => {}
                        }
                        w.events.push_back(e);
                        Ok(())
                    })?;
                }
            }
            let drained = with_win(id, |w| {
                let drained = std::mem::take(&mut w.events);
                for e in &drained {
                    w.canvas.feed(e);
                }
                Ok(drained)
            })?;
            let mut out = Vec::with_capacity(drained.len());
            for e in &drained {
                out.push(ui_event(ctx, e)?);
            }
            Ok(out)
        }
        fn rect(_ctx: &mut dyn NativeCtx, id: i64, x: f64, y: f64, w: f64, h: f64, color: i64, radius: f64) -> Result<(), String> {
            with_win(id, |win| {
                win.commands.push(DrawCmd::Rect { x, y, w, h, color, radius });
                Ok(())
            })
        }
        fn text(_ctx: &mut dyn NativeCtx, id: i64, x: f64, y: f64, s: &str, size: f64, color: i64) -> Result<(), String> {
            with_win(id, |win| {
                win.commands.push(DrawCmd::Text { x, y, s: s.to_string(), size, color });
                Ok(())
            })
        }
        fn clip(_ctx: &mut dyn NativeCtx, id: i64, x: f64, y: f64, w: f64, h: f64) -> Result<(), String> {
            with_win(id, |win| {
                win.commands.push(DrawCmd::Clip { x, y, w, h });
                Ok(())
            })
        }
        fn unclip(_ctx: &mut dyn NativeCtx, id: i64) -> Result<(), String> {
            with_win(id, |win| {
                win.commands.push(DrawCmd::Unclip);
                Ok(())
            })
        }
        fn clear(_ctx: &mut dyn NativeCtx, id: i64, color: i64) -> Result<(), String> {
            with_win(id, |win| {
                win.commands.push(DrawCmd::Clear(color));
                Ok(())
            })
        }
        fn present(_ctx: &mut dyn NativeCtx, id: i64) -> Result<i64, String> {
            with_win(id, |win| {
                let n = win.commands.len() as i64;
                let cmds = std::mem::take(&mut win.commands);
                let scene = ui_draw::assemble(&cmds)?;
                let hits = match win.real {
                    Some(h) => varn_ui::draw_now(h, &mut win.canvas, scene.clear, &scene.runs)
                        .unwrap_or_default(),
                    None => win.canvas.interact(win.w as u32, win.h as u32, &scene.runs),
                };
                win.activated.extend(hits.activations);
                win.typed.extend(hits.texts);
                Ok(n)
            })
        }
        fn control(
            ctx: &mut dyn NativeCtx,
            window: i64,
            cid: i64,
            kind: VmValue,
            x: f64,
            y: f64,
            w: f64,
            h: f64,
            color: i64,
            flag: bool,
            value: f64,
            low: f64,
            high: f64,
        ) -> Result<(), String> {
            let kind = match UiControlKind::from_value(ctx, kind)
                .ok_or_else(|| "ui.control: kind is not a UiControlKind".to_string())?
            {
                UiControlKind::Button => varn_ui::ControlKind::Button,
                UiControlKind::Checkbox => varn_ui::ControlKind::Checkbox,
                UiControlKind::Switch => varn_ui::ControlKind::Switch,
                UiControlKind::Slider => varn_ui::ControlKind::Slider,
                UiControlKind::Scroll => varn_ui::ControlKind::Scroll,
                UiControlKind::Surface => varn_ui::ControlKind::Surface,
                UiControlKind::Field | UiControlKind::Area => {
                    return Err("ui.control: use ui.field for text controls".to_string())
                }
            };
            let id = u32::try_from(cid).map_err(|_| "ui.control: bad control id".to_string())?;
            with_win(window, |win| {
                win.commands.push(DrawCmd::Control {
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
                    text: String::new(),
                    hint: String::new(),
                    size: 0.0,
                });
                Ok(())
            })
        }
        fn field(
            ctx: &mut dyn NativeCtx,
            window: i64,
            cid: i64,
            kind: VmValue,
            x: f64,
            y: f64,
            w: f64,
            h: f64,
            color: i64,
            focused: bool,
            text: &str,
            hint: &str,
            size: f64,
        ) -> Result<(), String> {
            let kind = match UiControlKind::from_value(ctx, kind)
                .ok_or_else(|| "ui.field: kind is not a UiControlKind".to_string())?
            {
                UiControlKind::Field => varn_ui::ControlKind::Field,
                UiControlKind::Area => varn_ui::ControlKind::Area,
                UiControlKind::Button | UiControlKind::Checkbox | UiControlKind::Switch | UiControlKind::Slider | UiControlKind::Scroll | UiControlKind::Surface => {
                    return Err("ui.field: kind is not a text control".to_string())
                }
            };
            let id = u32::try_from(cid).map_err(|_| "ui.field: bad control id".to_string())?;
            with_win(window, |win| {
                win.commands.push(DrawCmd::Control {
                    id,
                    kind,
                    x,
                    y,
                    w,
                    h,
                    color,
                    flag: focused,
                    value: 0.0,
                    low: 0.0,
                    high: 0.0,
                    text: text.to_string(),
                    hint: hint.to_string(),
                    size,
                });
                Ok(())
            })
        }
        fn typed(ctx: &mut dyn NativeCtx, id: i64) -> Result<Vec<VmValue>, String> {
            let changes = with_win(id, |win| Ok(std::mem::take(&mut win.typed)))?;
            Ok(changes
                .into_iter()
                .map(|c| {
                    let text = ctx.alloc_str_owned(c.text);
                    TypedText {
                        id: i64::from(c.id),
                        text,
                    }
                    .alloc(ctx)
                })
                .collect())
        }
        fn activated(_ctx: &mut dyn NativeCtx, id: i64) -> Result<Vec<VmValue>, String> {
            let hits = with_win(id, |win| Ok(std::mem::take(&mut win.activated)))?;
            Ok(hits
                .into_iter()
                .flat_map(|a| [VmValue::from_f64(f64::from(a.id)), VmValue::from_f64(f64::from(a.value))])
                .collect())
        }
        fn real(_ctx: &mut dyn NativeCtx, id: i64) -> Result<bool, String> {
            with_win(id, |w| Ok(w.real.is_some()))
        }
        fn keyCode(_ctx: &mut dyn NativeCtx, name: &str) -> Result<i64, String> {
            varn_ui::named_key_code(name)
                .ok_or_else(|| format!("ui.keyCode: unknown key '{name}'"))
        }
        fn drawn(ctx: &mut dyn NativeCtx, id: i64) -> Result<Vec<VmValue>, String> {
            let cmds = with_win(id, |win| Ok(win.commands.clone()))?;
            Ok(cmds
                .iter()
                .map(|c| {
                    let d = ui_draw::describe(c);
                    let kind = ctx.alloc_str(d.kind);
                    let text = ctx.alloc_str_owned(d.text);
                    DrawnItem {
                        kind,
                        x: d.x,
                        y: d.y,
                        w: d.w,
                        h: d.h,
                        color: d.color,
                        text,
                    }
                    .alloc(ctx)
                })
                .collect())
        }
    }
}
