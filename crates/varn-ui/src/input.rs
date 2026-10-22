use egui::{Event, Key, Modifiers, MouseWheelSource, MouseWheelUnit, PointerButton, Pos2, TouchPhase, Vec2};

use super::events::{
    RealEvent, KEY_ARROW_DOWN, KEY_ARROW_LEFT, KEY_ARROW_RIGHT, KEY_ARROW_UP, KEY_BACKSPACE,
    KEY_DELETE, KEY_ENTER, KEY_ESCAPE, KEY_SPACE, KEY_TAB,
};

fn named(key: i64) -> Option<Key> {
    Some(match key {
        KEY_BACKSPACE => Key::Backspace,
        KEY_TAB => Key::Tab,
        KEY_ENTER => Key::Enter,
        KEY_ESCAPE => Key::Escape,
        KEY_DELETE => Key::Delete,
        KEY_ARROW_LEFT => Key::ArrowLeft,
        KEY_ARROW_UP => Key::ArrowUp,
        KEY_ARROW_RIGHT => Key::ArrowRight,
        KEY_ARROW_DOWN => Key::ArrowDown,
        _ => return None,
    })
}

fn button(code: i64) -> Option<PointerButton> {
    match code {
        0 => Some(PointerButton::Primary),
        1 => Some(PointerButton::Secondary),
        2 => Some(PointerButton::Middle),
        _ => None,
    }
}

pub fn push(e: &RealEvent, out: &mut Vec<Event>) {
    match *e {
        RealEvent::Mouse { x, y, button: code, pressed } => {
            let pos = Pos2::new(x as f32, y as f32);
            out.push(Event::PointerMoved(pos));
            if let Some(button) = button(code) {
                out.push(Event::PointerButton {
                    pos,
                    button,
                    pressed,
                    modifiers: Modifiers::NONE,
                });
            }
        }
        RealEvent::Wheel { dy } => out.push(Event::MouseWheel {
            unit: MouseWheelUnit::Point,
            delta: Vec2::new(0.0, dy as f32),
            phase: TouchPhase::Move,
            source: MouseWheelSource::Wheel,
            modifiers: Modifiers::NONE,
        }),
        RealEvent::Key { key, pressed } => {
            if let Some(key) = named(key) {
                out.push(Event::Key {
                    key,
                    physical_key: None,
                    pressed,
                    repeat: false,
                    modifiers: Modifiers::NONE,
                });
            } else if pressed && key >= KEY_SPACE {
                if let Some(c) = char::from_u32(key as u32) {
                    out.push(Event::Text(c.to_string()));
                }
            }
        }
        RealEvent::Resize { .. } | RealEvent::Close => {}
    }
}
