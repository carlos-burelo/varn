use winit::event::{ElementState, KeyEvent, MouseButton};
use winit::keyboard::{Key, NamedKey};

pub enum RealEvent {
    Key {
        key: i64,
        pressed: bool,
    },
    Mouse {
        x: i64,
        y: i64,
        button: i64,
        pressed: bool,
    },
    Resize {
        w: i64,
        h: i64,
    },
    Close,
}

pub const KEY_BACKSPACE: i64 = 8;
pub const KEY_TAB: i64 = 9;
pub const KEY_ENTER: i64 = 13;
pub const KEY_ESCAPE: i64 = 27;
pub const KEY_SPACE: i64 = 32;
pub const KEY_DELETE: i64 = 127;
pub const KEY_ARROW_LEFT: i64 = 0x2190;
pub const KEY_ARROW_UP: i64 = 0x2191;
pub const KEY_ARROW_RIGHT: i64 = 0x2192;
pub const KEY_ARROW_DOWN: i64 = 0x2193;

const KEYS: [(NamedKey, i64, &str); 10] = [
    (NamedKey::Backspace, KEY_BACKSPACE, "backspace"),
    (NamedKey::Tab, KEY_TAB, "tab"),
    (NamedKey::Enter, KEY_ENTER, "enter"),
    (NamedKey::Escape, KEY_ESCAPE, "escape"),
    (NamedKey::Space, KEY_SPACE, "space"),
    (NamedKey::Delete, KEY_DELETE, "delete"),
    (NamedKey::ArrowLeft, KEY_ARROW_LEFT, "left"),
    (NamedKey::ArrowUp, KEY_ARROW_UP, "up"),
    (NamedKey::ArrowRight, KEY_ARROW_RIGHT, "right"),
    (NamedKey::ArrowDown, KEY_ARROW_DOWN, "down"),
];

fn named_code(key: NamedKey) -> Option<i64> {
    KEYS.iter().find_map(|(k, c, _)| (*k == key).then_some(*c))
}

pub fn named_key_code(name: &str) -> Option<i64> {
    KEYS.iter().find_map(|(_, c, n)| (*n == name).then_some(*c))
}

pub fn key_code(key: &KeyEvent) -> i64 {
    if let Key::Named(named) = &key.logical_key
        && let Some(code) = named_code(*named)
    {
        return code;
    }
    key.logical_key
        .to_text()
        .and_then(|s| s.chars().next())
        .map(|c| c as i64)
        .unwrap_or(0)
}

pub fn pressed(state: ElementState) -> bool {
    matches!(state, ElementState::Pressed)
}

pub fn button_code(b: MouseButton) -> i64 {
    match b {
        MouseButton::Left => 0,
        MouseButton::Right => 1,
        MouseButton::Middle => 2,
        MouseButton::Back => 3,
        MouseButton::Forward => 4,
        MouseButton::Other(n) => 10 + n as i64,
    }
}
