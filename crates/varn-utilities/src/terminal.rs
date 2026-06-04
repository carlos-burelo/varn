use crate::chalk::{chalk, Chalk};
use std::fmt::Display;

// ── Table ─────────────────────────────────────────────────────────────────────

pub struct Table {
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
    widths: Vec<usize>,
}

impl Table {
    pub fn new(headers: impl IntoIterator<Item = impl Into<String>>) -> Self {
        let headers: Vec<String> = headers.into_iter().map(Into::into).collect();
        let widths = headers.iter().map(|h| h.len()).collect();
        Self { headers, rows: Vec::new(), widths }
    }

    pub fn row(&mut self, cells: impl IntoIterator<Item = impl Into<String>>) -> &mut Self {
        let row: Vec<String> = cells.into_iter().map(Into::into).collect();
        for (i, cell) in row.iter().enumerate() {
            if let Some(w) = self.widths.get_mut(i) {
                *w = (*w).max(cell.len());
            }
        }
        self.rows.push(row);
        self
    }

    pub fn print(&self) {
        let header_line = self
            .headers
            .iter()
            .enumerate()
            .map(|(i, h)| chalk(format_args!("{:<w$}", h, w = self.widths[i])).dim().to_string())
            .collect::<Vec<_>>()
            .join(" │ ");
        eprintln!("  {header_line}");

        let sep: String = self.widths.iter().map(|w| "─".repeat(*w)).collect::<Vec<_>>().join("─┼─");
        eprintln!("  {sep}");

        for row in &self.rows {
            let line = row
                .iter()
                .enumerate()
                .map(|(i, cell)| {
                    if let Some(&w) = self.widths.get(i) {
                        format!("{:<w$}", cell)
                    } else {
                        cell.clone()
                    }
                })
                .collect::<Vec<_>>()
                .join(" │ ");
            eprintln!("  {line}");
        }
    }
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
