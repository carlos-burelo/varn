use std::time::Duration;

pub const LABEL_WIDTH: usize = 26;

pub const VALUE_WIDTH: usize = 10;

#[derive(Clone, Copy)]
pub struct DurScale {
    divisor: f64,
    suffix: &'static str,
    decimals: usize,
}

impl DurScale {
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

pub fn fmt_pct(ratio: f64) -> String {
    format!("{:.1}%", ratio * 100.0)
}

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

pub fn row(label: &str, value: impl AsRef<str>) -> String {
    format!(
        "  {:<LABEL_WIDTH$} {:>VALUE_WIDTH$}",
        truncate_middle(label, LABEL_WIDTH),
        value.as_ref()
    )
}

pub fn row_note(label: &str, value: impl AsRef<str>, note: impl AsRef<str>) -> String {
    use varn_core::term::chalk::chalk;
    format!("{}  {}", row(label, value), chalk(note.as_ref()).dim())
}
