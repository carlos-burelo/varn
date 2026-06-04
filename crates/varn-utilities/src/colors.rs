pub const RESET: &str = "\x1b[0m";
pub const BOLD: &str = "\x1b[1m";
pub const DIM: &str = "\x1b[2m";
pub const UNDERLINE: &str = "\x1b[4m";

pub const RED: &str = "\x1b[31m";
pub const GREEN: &str = "\x1b[32m";
pub const YELLOW: &str = "\x1b[33m";
pub const BLUE: &str = "\x1b[34m";
pub const MAGENTA: &str = "\x1b[35m";
pub const CYAN: &str = "\x1b[36m";

pub const C_ERRORS: &str = RED;
pub const C_TOKENS: &str = MAGENTA;
pub const C_AST: &str = CYAN;
pub const C_SYMBOLS: &str = GREEN;
pub const C_TYPES: &str = BLUE;
pub const C_BYTECODE: &str = YELLOW;
pub const C_SCOPE: &str = MAGENTA;
pub const C_MODULES: &str = CYAN;
pub const C_BINDS: &str = BLUE;
pub const C_CONSTS: &str = YELLOW;

pub const R: &str = RESET;

pub fn header(color: &str, title: &str, path: &str) {
    let padding = "─".repeat((50_isize - title.len() as isize - 1).max(0) as usize);
    eprintln!(
        "\n {color}{title} {R}{DIM}{padding} {path}{RESET}",
        color = color,
        title = title,
        R = R,
        DIM = DIM,
        padding = padding,
        path = path,
        RESET = RESET
    );
}

pub fn footer(color: &str, msg: &str) {
    eprintln!(
        "  {color}-- {msg} {RESET}\n",
        color = color,
        msg = msg,
        RESET = RESET
    );
}

use std::fmt::Display;

#[derive(Clone, Copy)]
pub enum Color {
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
    Bold,
    Dim,
}

impl Display for Color {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let color_str = match self {
            Color::Red => RED,
            Color::Green => GREEN,
            Color::Yellow => YELLOW,
            Color::Blue => BLUE,
            Color::Magenta => MAGENTA,
            Color::Cyan => CYAN,
            Color::White => "",
            Color::Bold => BOLD,
            Color::Dim => DIM,
        };
        write!(f, "{}", color_str)
    }
}

pub fn colored<D: Display>(text: D, color: Color) -> String {
    format!("{}{}{}", color, text, RESET)
}
