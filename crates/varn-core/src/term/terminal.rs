//! Tablas, secciones y avisos de los volcados `vn debug`.
//!
//! El render (ancho visible, truncado, relleno) y la escritura a stderr van
//! por [`console`]: `measure_text_width` entiende ANSI + Unicode ancho
//! (CJK/emoji), `pad_str`/`truncate_str` alinean sin romper secuencias, y
//! `Term::stderr` habilita ANSI en Windows. La API pública (`Table`,
//! `Section`, `log/warn/error/info`) no cambia.

use crate::term::chalk::{chalk, Chalk};
use std::fmt::Display;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Right,
}

enum Line {
    Cells(Vec<String>),
    Rule,
}

pub struct Table {
    headers: Vec<String>,
    aligns: Vec<Align>,
    lines: Vec<Line>,
    widths: Vec<usize>,
}

impl Table {
    pub fn new(headers: impl IntoIterator<Item = impl Into<String>>) -> Self {
        let headers: Vec<String> = headers.into_iter().map(Into::into).collect();
        let widths = headers.iter().map(|h| display_width(h)).collect();
        let aligns = vec![Align::Left; headers.len()];
        Self {
            headers,
            aligns,
            lines: Vec::new(),
            widths,
        }
    }

    pub fn align(mut self, aligns: impl IntoIterator<Item = Align>) -> Self {
        for (slot, a) in self.aligns.iter_mut().zip(aligns) {
            *slot = a;
        }
        self
    }

    pub fn row(&mut self, cells: impl IntoIterator<Item = impl Into<String>>) -> &mut Self {
        let row: Vec<String> = cells.into_iter().map(Into::into).collect();
        for (i, cell) in row.iter().enumerate() {
            if let Some(w) = self.widths.get_mut(i) {
                *w = (*w).max(display_width(cell));
            }
        }
        self.lines.push(Line::Cells(row));
        self
    }

    pub fn rule(&mut self) -> &mut Self {
        self.lines.push(Line::Rule);
        self
    }

    pub fn print(&self) {
        let header = self
            .headers
            .iter()
            .enumerate()
            .map(|(i, h)| chalk(self.pad(i, h)).dim().to_string())
            .collect::<Vec<_>>()
            .join(" │ ");
        write_line(format!("  {header}"));
        write_line(format!("  {}", self.rule_line()));

        for line in &self.lines {
            match line {
                Line::Rule => write_line(format!("  {}", self.rule_line())),
                Line::Cells(row) => {
                    let line = row
                        .iter()
                        .enumerate()
                        .map(|(i, cell)| self.pad(i, cell))
                        .collect::<Vec<_>>()
                        .join(" │ ");
                    write_line(format!("  {line}"));
                }
            }
        }
    }

    fn pad(&self, col: usize, text: &str) -> String {
        let width = self
            .widths
            .get(col)
            .copied()
            .unwrap_or_else(|| display_width(text));
        let align = self.aligns.get(col).copied().unwrap_or(Align::Left);
        let console_align = match align {
            Align::Left => console::Alignment::Left,
            Align::Right => console::Alignment::Right,
        };
        console::pad_str(text, width, console_align, None).into_owned()
    }

    fn rule_line(&self) -> String {
        self.widths
            .iter()
            .map(|w| "─".repeat(*w))
            .collect::<Vec<_>>()
            .join("─┼─")
    }
}

/// Ancho visible en terminal: ignora secuencias ANSI y cuenta los caracteres
/// anchos (CJK/emoji) como 2. La versión anterior contaba `chars()` (todo = 1)
/// y desalineaba tablas con contenido no ASCII.
fn display_width(s: &str) -> usize {
    console::measure_text_width(s)
}

pub struct Section {
    title: String,
    subtitle: String,
    color: fn(Chalk) -> Chalk,
}

impl Section {
    pub fn new(title: impl Display) -> Self {
        Self {
            title: title.to_string(),
            subtitle: String::new(),
            color: |c| c,
        }
    }

    pub fn subtitle(mut self, sub: impl Display) -> Self {
        self.subtitle = sub.to_string();
        self
    }

    pub fn color(mut self, f: fn(Chalk) -> Chalk) -> Self {
        self.color = f;
        self
    }

    pub fn print(&self) {
        let pad_len = (50_isize - self.title.len() as isize - 1).max(0) as usize;
        let padding = "─".repeat(pad_len);
        let title = (self.color)(chalk(&self.title));
        write_line(format!(
            "\n  {title} {}",
            chalk(format_args!("{padding} {}", self.subtitle)).dim()
        ));
    }

    pub fn close(&self) {
        write_line(format!(
            "  {}",
            chalk(format_args!("── end: {} ──", self.title)).dim()
        ));
    }
}

/// Una línea a stderr. Va por `Term::stderr` para habilitar ANSI en Windows;
/// si falla (stderr cerrado), cae a `eprintln!` para no perder el volcado.
fn write_line(line: String) {
    let term = console::Term::stderr();
    if term.write_line(&line).is_err() {
        eprintln!("{line}");
    }
}

pub fn log(msg: impl Display) {
    write_line(msg.to_string());
}

/// Avisos humanos (no volcados): con detección automática de color.
/// Respetan `NO_COLOR`, `CLICOLOR` y TTY; en tuberías salen sin ANSI.
pub fn warn(msg: impl Display) {
    let tag = console::style("warn")
        .yellow()
        .bold()
        .for_stderr()
        .to_string();
    write_line(format!("  {tag} {msg}"));
}

pub fn error(msg: impl Display) {
    let tag = console::style("error")
        .red()
        .bold()
        .for_stderr()
        .to_string();
    write_line(format!("  {tag} {msg}"));
}

pub fn info(msg: impl Display) {
    let tag = console::style("info").cyan().dim().for_stderr().to_string();
    write_line(format!("  {tag} {msg}"));
}

pub fn blank() {
    write_line(String::new());
}

pub fn tagged(tag: impl Display, msg: impl Display) {
    let tag = console::style(tag.to_string())
        .dim()
        .for_stderr()
        .to_string();
    write_line(format!("[{tag}] {msg}"));
}

pub fn separator() {
    write_line(format!("  {}", "─".repeat(50)));
}
