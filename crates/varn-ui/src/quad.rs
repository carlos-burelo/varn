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
