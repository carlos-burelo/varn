//! The single copy of every formatter the bench report uses.
//!
//! Durations are formatted two ways, and the distinction matters for
//! readability:
//!
//! * [`fmt_dur`] picks a unit per value. Fine for a lone number in prose.
//! * [`DurScale`] picks one unit for a whole *column* and holds the decimal
//!   count fixed across it. Per-cell units put `8.82 µs` above `202 µs` above
//!   `37.87 ms`, which defeats vertical scanning — the reader has to re-read
//!   the suffix on every row to compare magnitudes.

use std::time::Duration;

/// Column label width shared by every non-tabular section, so the VM, GC, JIT
/// and hotspot blocks line up as one report instead of four.
pub const LABEL_WIDTH: usize = 26;

/// Value column width for those same sections.
pub const VALUE_WIDTH: usize = 10;

/// A unit choice fixed for one column of durations.
#[derive(Clone, Copy)]
pub struct DurScale {
    divisor: f64,
    suffix: &'static str,
    decimals: usize,
}

impl DurScale {
    /// Choose the unit from the largest value in the column, then fix the
    /// decimals so every cell has the same precision.
    pub fn for_column(values: impl IntoIterator<Item = Duration>) -> Self {
        let max_ns = values
            .into_iter()
            .map(|d| d.as_nanos())
            .max()
            .unwrap_or(0)
            .max(1) as f64;

        if max_ns < 1_000.0 {
            Self {
                divisor: 1.0,
                suffix: "ns",
                decimals: 0,
            }
        } else if max_ns < 1_000_000.0 {
            Self {
                divisor: 1_000.0,
                suffix: "µs",
                decimals: 1,
            }
        } else if max_ns < 1_000_000_000.0 {
            Self {
                divisor: 1_000_000.0,
                suffix: "ms",
                decimals: 2,
            }
        } else {
            Self {
                divisor: 1_000_000_000.0,
                suffix: "s",
                decimals: 3,
            }
        }
    }

    pub fn fmt(&self, d: Duration) -> String {
        let v = d.as_nanos() as f64 / self.divisor;
        format!(
            "{v:.prec$} {suffix}",
            prec = self.decimals,
            suffix = self.suffix
        )
    }
}

/// Format one duration on its own, choosing the unit from its own magnitude.
pub fn fmt_dur(d: Duration) -> String {
    let ns = d.as_nanos();
    if ns < 1_000 {
        format!("{ns} ns")
    } else if ns < 1_000_000 {
        format!("{:.1} µs", ns as f64 / 1_000.0)
    } else if ns < 1_000_000_000 {
        format!("{:.2} ms", ns as f64 / 1_000_000.0)
    } else {
        format!("{:.3} s", ns as f64 / 1_000_000_000.0)
    }
}

/// Thin-space grouped integer: `31 726`.
pub fn fmt_num(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(' ');
        }
        out.push(c);
    }
    out.chars().rev().collect()
}

pub fn fmt_bytes(n: u64) -> String {
    if n < 1_024 {
        format!("{n} B")
    } else if n < 1_048_576 {
        format!("{:.1} KB", n as f64 / 1_024.0)
    } else {
        format!("{:.2} MB", n as f64 / 1_048_576.0)
    }
}

/// Percentage with one decimal. `ratio` is 0.0..=1.0.
pub fn fmt_pct(ratio: f64) -> String {
    format!("{:.1}%", ratio * 100.0)
}

/// Strip Windows' extended-length prefix and shorten to a path relative to the
/// current directory when the file sits underneath it. `\\?\C:\a\b\main.vn`
/// under `C:\a` becomes `b\main.vn`.
pub fn short_path(path: &str) -> String {
    let stripped = path
        .strip_prefix(r"\\?\UNC\")
        .map(|rest| format!(r"\\{rest}"))
        .unwrap_or_else(|| path.strip_prefix(r"\\?\").unwrap_or(path).to_owned());

    let Ok(cwd) = std::env::current_dir() else {
        return stripped;
    };
    let cwd = cwd.to_string_lossy();
    let cwd = cwd.strip_prefix(r"\\?\").unwrap_or(&cwd);

    match stripped.strip_prefix(cwd) {
        Some(rest) => rest.trim_start_matches(['/', '\\']).to_owned(),
        None => stripped,
    }
}

/// Drop the directory part and the extension from a module-qualified global
/// name, so `C:/…/tests/31-stdlib-migration-test.vn::hash` reads as
/// `31-stdlib-migration-test::hash`. Names without a `::` are returned as-is.
pub fn short_global(name: &str) -> String {
    let Some((module, symbol)) = name.rsplit_once("::") else {
        return name.to_owned();
    };
    let file = module
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(module)
        .trim_end_matches(".vn");
    format!("{file}::{symbol}")
}

/// Shorten to `width` by elliding the middle, which keeps both the
/// distinguishing prefix and the symbol at the end. Truncating the tail would
/// collapse every global from one module to the same string.
pub fn truncate_middle(s: &str, width: usize) -> String {
    let count = s.chars().count();
    if count <= width {
        return s.to_owned();
    }
    if width <= 1 {
        return "…".to_owned();
    }
    let keep = width - 1;
    let head = keep.div_ceil(2);
    let tail = keep - head;
    let head_str: String = s.chars().take(head).collect();
    let tail_str: String = s.chars().skip(count - tail).collect();
    format!("{head_str}…{tail_str}")
}

/// `label` padded to [`LABEL_WIDTH`], `value` right-aligned to [`VALUE_WIDTH`].
pub fn row(label: &str, value: impl AsRef<str>) -> String {
    format!(
        "  {:<LABEL_WIDTH$} {:>VALUE_WIDTH$}",
        truncate_middle(label, LABEL_WIDTH),
        value.as_ref()
    )
}

/// [`row`] plus a dim trailing note.
pub fn row_note(label: &str, value: impl AsRef<str>, note: impl AsRef<str>) -> String {
    use varn_core::term::chalk::chalk;
    format!("{}  {}", row(label, value), chalk(note.as_ref()).dim())
}