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

struct Event {
    kind: UiEventKind,
    key: i64,
    pressed: bool,
    x: i64,
    y: i64,
    button: i64,
    w: i64,
    h: i64,
}

struct Window {
    alive: bool,
    closed: bool,
    w: i64,
    h: i64,
    events: VecDeque<Event>,
    commands: Vec<DrawCmd>,
    real: Option<u64>,
}

fn real_poll(id: u64) -> Vec<Event> {
    let mut out = Vec::new();
    let Ok(evs) = varn_ui::take_events(id) else {
        return out;
    };
    for e in evs {
        out.push(match e {
            varn_ui::RealEvent::Key { key, pressed } => Event {
                kind: UiEventKind::Key,
                key,
                pressed,
                x: 0,
                y: 0,
                button: 0,
                w: 0,
                h: 0,
            },
            varn_ui::RealEvent::Mouse {
                x,
                y,
                button,
                pressed,
            } => Event {
                kind: UiEventKind::Mouse,
                key: 0,
                pressed,
                x,
                y,
                button,
                w: 0,
                h: 0,
            },
            varn_ui::RealEvent::Resize { w, h } => Event {
                kind: UiEventKind::Resize,
                key: 0,
                pressed: false,
                x: 0,
                y: 0,
                button: 0,
                w,
                h,
            },
            varn_ui::RealEvent::Close => Event {
                kind: UiEventKind::Close,
                key: 0,
                pressed: false,
                x: 0,
                y: 0,
                button: 0,
                w: 0,
                h: 0,
            },
        });
    }
    out
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

fn event_obj(ctx: &mut dyn NativeCtx, e: &Event) -> Result<VmValue, String> {
    let kind = e.kind.to_value(ctx)?;
    Ok(UiEvent {
        kind,
        key: e.key,
        pressed: e.pressed,
        x: e.x,
        y: e.y,
        button: e.button,
        w: e.w,
        h: e.h,
    }
    .alloc(ctx))
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
        fn open(_ctx: &mut dyn NativeCtx, title: &str, w: i64, h: i64) -> Result<i64, String> {
            if w <= 0 || h <= 0 {
                return Err(format!("ui.open: bad size ({w}x{h})"));
            }
            let mut all = windows().lock();
            let real = if w <= u32::MAX as i64 && h <= u32::MAX as i64 {
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
                    UiInjectKind::Key => Event { kind: UiEventKind::Key, key: a, pressed: b != 0, x: 0, y: 0, button: 0, w: 0, h: 0 },
                    UiInjectKind::Mouse => Event { kind: UiEventKind::Mouse, key: 0, pressed: false, x: a, y: b, button: c, w: 0, h: 0 },
                    UiInjectKind::MouseDown => Event { kind: UiEventKind::Mouse, key: 0, pressed: true, x: a, y: b, button: c, w: 0, h: 0 },
                    UiInjectKind::MouseUp => Event { kind: UiEventKind::Mouse, key: 0, pressed: false, x: a, y: b, button: c, w: 0, h: 0 },
                    UiInjectKind::Resize => {
                        w.w = a;
                        w.h = b;
                        Event { kind: UiEventKind::Resize, key: 0, pressed: false, x: 0, y: 0, button: 0, w: a, h: b }
                    }
                    UiInjectKind::Close => {
                        w.closed = true;
                        Event { kind: UiEventKind::Close, key: 0, pressed: false, x: 0, y: 0, button: 0, w: 0, h: 0 }
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
                    let is_close = e.kind == UiEventKind::Close;
                    let is_resize = e.kind == UiEventKind::Resize;
                    let (rw, rh) = (e.w, e.h);
                    with_win(id, |w| {
                        if is_close {
                            w.closed = true;
                        }
                        if is_resize {
                            w.w = rw;
                            w.h = rh;
                        }
                        w.events.push_back(e);
                        Ok(())
                    })?;
                }
            }
            let drained = with_win(id, |w| Ok(std::mem::take(&mut w.events)))?;
            let mut out = Vec::with_capacity(drained.len());
            for e in &drained {
                out.push(event_obj(ctx, e)?);
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
            let (n, real, cmds) = with_win(id, |win| {
                let n = win.commands.len() as i64;
                let cmds = std::mem::take(&mut win.commands);
                Ok((n, win.real, cmds))
            })?;
            if let Some(h) = real {
                let scene = ui_draw::assemble(&cmds)?;
                let atlas = scene.atlas.as_ref().map(|(b, w, h)| (b.as_slice(), *w, *h));
                let _ = varn_ui::draw_now(h, scene.clear, &scene.runs, atlas);
            }
            Ok(n)
        }
        fn real(_ctx: &mut dyn NativeCtx, id: i64) -> Result<bool, String> {
            with_win(id, |w| Ok(w.real.is_some()))
        }
        fn keyCode(_ctx: &mut dyn NativeCtx, name: &str) -> Result<i64, String> {
            varn_ui::named_key_code(name)
                .ok_or_else(|| format!("ui.keyCode: unknown key '{name}'"))
        }
        fn commands(ctx: &mut dyn NativeCtx, id: i64) -> Result<Vec<VmValue>, String> {
            let cmds = with_win(id, |win| Ok(win.commands.clone()))?;
            Ok(cmds.iter().map(|c| ctx.alloc_str_owned(ui_draw::format(c))).collect())
        }
    }
}
