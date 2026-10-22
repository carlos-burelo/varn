use std::cell::RefCell;
use std::time::Duration;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::platform::pump_events::{EventLoopExtPumpEvents, PumpStatus};
use winit::window::{Window, WindowAttributes, WindowId};

use super::events::{button_code, key_code, pressed, RealEvent};
use super::render;

struct WindowState {
    win_ptr: *mut Window,
    surface: Option<wgpu::Surface<'static>>,
    queue: Vec<RealEvent>,
    size: (u32, u32),
    configured: Option<(u32, u32)>,
    presented: bool,
    owner: std::thread::ThreadId,
    last_pos: (i64, i64),
    last_draw: Option<DrawnFrame>,
}

impl WindowState {
    fn win(&self) -> &Window {
        unsafe { &*self.win_ptr }
    }
}

#[derive(Clone)]
struct DrawnFrame {
    clear: [f32; 4],
    runs: Vec<super::comp::Run>,
    atlas: Option<(Vec<u8>, u32, u32)>,
}

struct PendingCreate {
    slot: usize,
    title: String,
    w: u32,
    h: u32,
}

struct UiState {
    eloop: Option<EventLoop<()>>,
    wins: Vec<Option<Box<WindowState>>>,
    free: Vec<usize>,
    pending: Vec<PendingCreate>,
}

thread_local! {
    static UI: RefCell<UiState> = const {
        RefCell::new(UiState {
            eloop: None,
            wins: Vec::new(),
            free: Vec::new(),
            pending: Vec::new(),
        })
    };
}

fn with_ui<T>(f: impl FnOnce(&mut UiState) -> T) -> T {
    UI.with(|ui| f(&mut ui.borrow_mut()))
}

#[cfg(windows)]
fn new_event_loop() -> Option<EventLoop<()>> {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    EventLoop::builder().with_any_thread(true).build().ok()
}

#[cfg(not(windows))]
fn new_event_loop() -> Option<EventLoop<()>> {
    None
}

pub fn try_open(title: &str, w: u32, h: u32) -> Option<u64> {
    if std::env::var("VARN_UI_REAL").as_deref() != Ok("1") {
        return None;
    }
    if w == 0 || h == 0 {
        return None;
    }
    let idx = with_ui(|ui| {
        if ui.eloop.is_none() {
            let eloop = std::panic::catch_unwind(new_event_loop)
                .ok()
                .and_then(|r| r)
                .or_else(|| {
                    std::panic::catch_unwind(EventLoop::new)
                        .ok()
                        .and_then(|r| r.ok())
                })?;
            ui.eloop = Some(eloop);
        }
        let idx = ui.free.pop().unwrap_or_else(|| {
            ui.wins.push(None);
            ui.wins.len() - 1
        });
        ui.pending.push(PendingCreate {
            slot: idx,
            title: title.to_string(),
            w,
            h,
        });
        Some(idx as u64)
    })?;
    pump_loop().ok();
    with_ui(|ui| {
        if ui.wins.get(idx as usize).and_then(|s| s.as_ref()).is_some() {
            Some(idx)
        } else {
            ui.free.push(idx as usize);
            None
        }
    })
}

fn check_owner(st: &WindowState) -> Result<(), String> {
    if st.owner != std::thread::current().id() {
        return Err("ui: window used from another thread".to_string());
    }
    Ok(())
}

pub fn pump(handle: u64) -> Result<(), String> {
    with_ui(|ui| {
        let st = ui
            .wins
            .get(handle as usize)
            .and_then(|s| s.as_ref())
            .ok_or_else(|| "ui: destroyed window".to_string())?;
        check_owner(st)?;
        Ok::<(), String>(())
    })?;
    pump_loop()
}

fn pump_loop() -> Result<(), String> {
    let mut eloop = with_ui(|ui| ui.eloop.take()).ok_or_else(|| "ui: no event loop".to_string())?;
    let mut app = PumpApp {};
    let r = eloop.pump_app_events(Some(Duration::ZERO), &mut app);
    with_ui(|ui| {
        ui.eloop = Some(eloop);
    });
    match r {
        PumpStatus::Continue => Ok(()),
        PumpStatus::Exit(_) => Err("ui: event loop exited".to_string()),
    }
}

pub fn take_events(handle: u64) -> Result<Vec<RealEvent>, String> {
    with_ui(|ui| {
        let st = ui
            .wins
            .get_mut(handle as usize)
            .and_then(|s| s.as_mut())
            .ok_or_else(|| "ui: destroyed window".to_string())?;
        check_owner(st)?;
        Ok(std::mem::take(&mut st.queue))
    })
}

fn ensure_surface(shared: &super::render::Shared, st: &mut WindowState) -> Result<(), String> {
    if st.surface.is_none() {
        st.surface = super::render::make_surface(shared, st.win());
        if st.surface.is_none() {
            return Err("ui: surface failed".to_string());
        }
    }
    Ok(())
}

pub fn draw_now(
    handle: u64,
    clear: [f32; 4],
    runs: &[super::comp::Run],
    atlas: Option<(&[u8], u32, u32)>,
) -> Result<bool, String> {
    with_ui(|ui| {
        let st = ui
            .wins
            .get_mut(handle as usize)
            .and_then(|s| s.as_mut())
            .ok_or_else(|| "ui: destroyed window".to_string())?;
        check_owner(st)?;
        let shared = super::render::shared().ok_or_else(|| "ui: no gpu device".to_string())?;
        ensure_surface(&shared, st)?;
        let (size, surface) = (st.size, st.surface.as_ref());
        let surface = surface.ok_or_else(|| "ui: surface failed".to_string())?;
        let mut configured = st.configured;
        let ok = super::comp::present_draw(
            &shared,
            surface,
            size.0,
            size.1,
            &mut configured,
            clear,
            runs,
            atlas,
        );
        let st = ui
            .wins
            .get_mut(handle as usize)
            .and_then(|s| s.as_mut())
            .ok_or_else(|| "ui: destroyed window".to_string())?;
        st.configured = configured;
        st.presented = ok;
        if ok {
            st.last_draw = Some(DrawnFrame {
                clear,
                runs: runs.to_vec(),
                atlas: atlas.map(|(b, w, h)| (b.to_vec(), w, h)),
            });
        }
        Ok(ok)
    })
}

pub fn size(handle: u64) -> Result<(u32, u32), String> {
    with_ui(|ui| {
        let st = ui
            .wins
            .get(handle as usize)
            .and_then(|s| s.as_ref())
            .ok_or_else(|| "ui: destroyed window".to_string())?;
        check_owner(st)?;
        Ok(st.size)
    })
}

pub fn close(handle: u64) -> Result<(), String> {
    let ptr = with_ui(|ui| {
        let slot = ui
            .wins
            .get_mut(handle as usize)
            .ok_or_else(|| "ui: destroyed window".to_string())?;
        let st = slot
            .as_mut()
            .ok_or_else(|| "ui: destroyed window".to_string())?;
        check_owner(st)?;
        st.surface = None;
        let ptr = st.win_ptr;
        st.win_ptr = std::ptr::null_mut();
        *slot = None;
        ui.free.push(handle as usize);
        Ok::<*mut Window, String>(ptr)
    })?;
    if !ptr.is_null() {
        unsafe {
            drop(Box::from_raw(ptr));
        }
    }
    Ok(())
}

struct PumpApp {}

impl ApplicationHandler for PumpApp {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        el.set_control_flow(ControlFlow::Wait);
        UI.with(|ui| {
            let mut ui = ui.borrow_mut();
            for p in std::mem::take(&mut ui.pending) {
                let attrs = WindowAttributes::default()
                    .with_title(p.title.as_str())
                    .with_inner_size(PhysicalSize::new(p.w, p.h));
                let Ok(window) = el.create_window(attrs) else {
                    ui.free.push(p.slot);
                    continue;
                };
                let s = window.inner_size();
                let win_ptr = Box::into_raw(Box::new(window));
                ui.wins[p.slot] = Some(Box::new(WindowState {
                    win_ptr,
                    surface: None,
                    queue: Vec::new(),
                    size: (s.width, s.height),
                    configured: None,
                    presented: false,
                    owner: std::thread::current().id(),
                    last_pos: (0, 0),
                    last_draw: None,
                }));
            }
        });
    }

    fn window_event(&mut self, _el: &ActiveEventLoop, id: WindowId, ev: WindowEvent) {
        UI.with(|ui| {
            let mut ui = ui.borrow_mut();
            let Some(st) = ui.wins.iter_mut().flatten().find(|st| st.win().id() == id) else {
                return;
            };
            match ev {
                WindowEvent::CloseRequested => st.queue.push(RealEvent::Close),
                WindowEvent::KeyboardInput { event, .. } => {
                    let k = key_code(&event);
                    if k >= 0 {
                        st.queue.push(RealEvent::Key {
                            key: k,
                            pressed: pressed(event.state),
                        });
                    }
                }
                WindowEvent::CursorMoved { position, .. } => {
                    st.last_pos = (position.x as i64, position.y as i64);
                    st.queue.push(RealEvent::Mouse {
                        x: st.last_pos.0,
                        y: st.last_pos.1,
                        button: -1,
                        pressed: false,
                    });
                }
                WindowEvent::MouseInput { button, state, .. } => {
                    st.queue.push(RealEvent::Mouse {
                        x: st.last_pos.0,
                        y: st.last_pos.1,
                        button: button_code(button),
                        pressed: pressed(state),
                    });
                }
                WindowEvent::Resized(s) => {
                    st.size = (s.width, s.height);
                    st.queue.push(RealEvent::Resize {
                        w: s.width as i64,
                        h: s.height as i64,
                    });
                }
                WindowEvent::RedrawRequested => {
                    if let Some(frame) = st.last_draw.clone()
                        && let Some(shared) = render::shared()
                    {
                        if st.surface.is_none() {
                            let ptr = st.win_ptr;
                            st.surface = render::make_surface(&shared, unsafe { &*ptr });
                        }
                        let atlas = frame.atlas.as_ref().map(|(b, w, h)| (b.as_slice(), *w, *h));
                        if st.surface.is_some() {
                            let size = st.size;
                            let configured = &mut st.configured;
                            if let Some(surface) = st.surface.as_ref() {
                                st.presented = super::comp::present_draw(
                                    &shared,
                                    surface,
                                    size.0,
                                    size.1,
                                    configured,
                                    frame.clear,
                                    &frame.runs,
                                    atlas,
                                );
                            }
                        }
                    }
                }
                WindowEvent::ActivationTokenDone { .. }
                | WindowEvent::Moved(_)
                | WindowEvent::Destroyed
                | WindowEvent::DroppedFile(_)
                | WindowEvent::HoveredFile(_)
                | WindowEvent::HoveredFileCancelled
                | WindowEvent::Focused(_)
                | WindowEvent::ModifiersChanged(_)
                | WindowEvent::Ime(_)
                | WindowEvent::CursorEntered { .. }
                | WindowEvent::CursorLeft { .. }
                | WindowEvent::MouseWheel { .. }
                | WindowEvent::PinchGesture { .. }
                | WindowEvent::PanGesture { .. }
                | WindowEvent::DoubleTapGesture { .. }
                | WindowEvent::RotationGesture { .. }
                | WindowEvent::TouchpadPressure { .. }
                | WindowEvent::AxisMotion { .. }
                | WindowEvent::Touch(_)
                | WindowEvent::ScaleFactorChanged { .. }
                | WindowEvent::ThemeChanged(_)
                | WindowEvent::Occluded(_) => {}
            }
        });
    }
}
