use crate::chalk::{chalk, Chalk};
use std::fmt::Display;

// ── Table ─────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Right,
}

enum Line {
    Cells(Vec<String>),
    Rule,
}

/// Aligned, color-aware text table.
///
/// Cells may contain ANSI styling (e.g. `chalk(...).cyan()`); column widths are
/// computed from the *visible* width so styling never breaks alignment.
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
        Self { headers, aligns, lines: Vec::new(), widths }
    }

    /// Set per-column alignment. Shorter than the header count keeps the rest `Left`.
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

    /// Insert a horizontal rule between rows (e.g. above a totals row).
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
        eprintln!("  {header}");
        eprintln!("  {}", self.rule_line());

        for line in &self.lines {
            match line {
                Line::Rule => eprintln!("  {}", self.rule_line()),
                Line::Cells(row) => {
                    let line = row
                        .iter()
                        .enumerate()
                        .map(|(i, cell)| self.pad(i, cell))
                        .collect::<Vec<_>>()
                        .join(" │ ");
                    eprintln!("  {line}");
                }
            }
        }
    }

    fn pad(&self, col: usize, text: &str) -> String {
        let width = self.widths.get(col).copied().unwrap_or_else(|| display_width(text));
        let align = self.aligns.get(col).copied().unwrap_or(Align::Left);
        let fill = width.saturating_sub(display_width(text));
        match align {
            Align::Left => format!("{text}{}", " ".repeat(fill)),
            Align::Right => format!("{}{text}", " ".repeat(fill)),
        }
    }

    fn rule_line(&self) -> String {
        self.widths.iter().map(|w| "─".repeat(*w)).collect::<Vec<_>>().join("─┼─")
    }
}

/// Visible width of `s`, ignoring ANSI escape sequences and counting each
/// remaining `char` as one column.
fn display_width(s: &str) -> usize {
    let mut width = 0;
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip a CSI sequence: `\x1b[ ... <final letter>`.
            for esc in chars.by_ref() {
                if esc.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            width += 1;
        }
    }
    width
}

// ── Section ───────────────────────────────────────────────────────────────────

pub struct Section {
    title: String,
    subtitle: String,
    color: fn(Chalk) -> Chalk,
}

impl Section {
    pub fn new(title: impl Display) -> Self {
        Self { title: title.to_string(), subtitle: String::new(), color: |c| c }
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
        eprintln!("\n  {title} {}", chalk(format_args!("{padding} {}", self.subtitle)).dim());
    }

    pub fn close(&self) {
        eprintln!("  {}", chalk(format_args!("── end: {} ──", self.title)).dim());
    }
}

// ── Output functions ──────────────────────────────────────────────────────────

pub fn log(msg: impl Display) {
    eprintln!("{msg}");
}

pub fn warn(msg: impl Display) {
    eprintln!("  {} {msg}", chalk("warn").yellow().bold());
}

pub fn error(msg: impl Display) {
    eprintln!("  {} {msg}", chalk("error").red().bold());
}

pub fn info(msg: impl Display) {
    eprintln!("  {} {msg}", chalk("info").cyan().dim());
}

pub fn blank() {
    eprintln!();
}

pub fn tagged(tag: impl Display, msg: impl Display) {
    eprintln!("[{}] {msg}", chalk(tag).dim());
}

pub fn separator() {
    eprintln!("  {}", "─".repeat(50));
}
