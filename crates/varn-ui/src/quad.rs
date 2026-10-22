#[derive(Clone)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub color: [f32; 4],
    pub radius: f32,
}

#[derive(Clone, Copy, PartialEq)]
pub struct Clip {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Clone)]
pub enum Body {
    Rects(Vec<Rect>),
    Glyphs(Vec<Glyph>),
    Control(Control),
}

#[derive(Clone)]
pub struct Run {
    pub clip: Option<Clip>,
    pub body: Body,
}

#[derive(Clone)]
pub struct Glyph {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub u: f32,
    pub v: f32,
    pub uw: f32,
    pub vh: f32,
    pub color: [f32; 4],
}

pub fn argb_to_float(color: i64) -> [f32; 4] {
    let u = color as u64;
    let ch = |shift: u32| (((u >> shift) & 0xff) as u8) as f32 / 255.0;
    [ch(16), ch(8), ch(0), ch(24)]
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ControlKind {
    Button,
    Checkbox,
    Switch,
    Slider,
    Scroll,
    Surface,
}

impl ControlKind {
    pub fn from_code(code: i64) -> Option<Self> {
        match code {
            0 => Some(Self::Button),
            1 => Some(Self::Checkbox),
            2 => Some(Self::Switch),
            3 => Some(Self::Slider),
            4 => Some(Self::Scroll),
            5 => Some(Self::Surface),
            _ => None,
        }
    }
}

#[derive(Clone)]
pub struct Control {
    pub id: u32,
    pub kind: ControlKind,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub color: [f32; 4],
    pub flag: bool,
    pub value: f32,
    pub low: f32,
    pub high: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct Activation {
    pub id: u32,
    pub value: f32,
}
