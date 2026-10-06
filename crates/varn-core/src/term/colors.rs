








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
    let line = format!("\n {color}{title} {R}{DIM}{padding} {path}{RESET}");
    let term = console::Term::stderr();
    if term.write_line(&line).is_err() {
        eprintln!("{line}");
    }
}

pub fn footer(color: &str, msg: &str) {
    let line = format!("  {color}-- {msg} {RESET}\n");
    let term = console::Term::stderr();
    if term.write_line(&line).is_err() {
        eprintln!("{line}");
    }
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

fn style_for(color: Color) -> console::Style {
    match color {
        Color::Red => console::Style::new().red(),
        Color::Green => console::Style::new().green(),
        Color::Yellow => console::Style::new().yellow(),
        Color::Blue => console::Style::new().blue(),
        Color::Magenta => console::Style::new().magenta(),
        Color::Cyan => console::Style::new().cyan(),
        Color::White => console::Style::new().white(),
        Color::Bold => console::Style::new().bold(),
        Color::Dim => console::Style::new().dim(),
    }
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
        write!(f, "{color_str}")
    }
}



pub fn colored<D: Display>(text: D, color: Color) -> String {
    style_for(color)
        .force_styling(true)
        .for_stderr()
        .apply_to(text.to_string())
        .to_string()
}
