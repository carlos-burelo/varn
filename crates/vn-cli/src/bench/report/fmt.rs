use std::time::Duration;

#[derive(Clone, Copy)]
pub struct DurScale {
    divisor: f64,
    unit: &'static str,
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
                unit: "ns",
                decimals: 0,
            }
        } else if max_ns < 1_000_000.0 {
            Self {
                divisor: 1_000.0,
                unit: "us",
                decimals: 2,
            }
        } else if max_ns < 1_000_000_000.0 {
            Self {
                divisor: 1_000_000.0,
                unit: "ms",
                decimals: 3,
            }
        } else {
            Self {
                divisor: 1_000_000_000.0,
                unit: "s",
                decimals: 4,
            }
        }
    }

    pub fn unit(&self) -> &'static str {
        self.unit
    }

    pub fn num(&self, d: Duration) -> String {
        format!(
            "{:.prec$}",
            d.as_nanos() as f64 / self.divisor,
            prec = self.decimals
        )
    }

    pub fn header(&self, name: &str) -> String {
        format!("{name}[{}]", self.unit)
    }
}

pub fn fmt_dur(d: Duration) -> String {
    let scale = DurScale::for_column([d]);
    format!("{} {}", scale.num(d), scale.unit())
}

pub fn fmt_bytes(n: u64) -> String {
    if n < 1_024 {
        format!("{n} B")
    } else if n < 1_048_576 {
        format!("{:.1} KiB", n as f64 / 1_024.0)
    } else {
        format!("{:.2} MiB", n as f64 / 1_048_576.0)
    }
}

pub fn percent(ratio: f64) -> String {
    format!("{:.1}", ratio * 100.0)
}

pub fn share(part: u64, whole: u64) -> String {
    if whole == 0 {
        "-".to_owned()
    } else {
        percent(part as f64 / whole as f64)
    }
}

pub fn short_path(path: &str) -> String {
    let stripped = path
        .strip_prefix(r"\\?\UNC\")
        .map(|rest| format!(r"\\{rest}"))
        .unwrap_or_else(|| path.strip_prefix(r"\\?\").unwrap_or(path).to_owned());

    let Ok(cwd) = std::env::current_dir() else {
        return stripped.replace('\\', "/");
    };
    let cwd = cwd.to_string_lossy();
    let cwd = cwd.strip_prefix(r"\\?\").unwrap_or(&cwd);

    match stripped.strip_prefix(cwd) {
        Some(rest) => rest.trim_start_matches(['/', '\\']).replace('\\', "/"),
        None => stripped.replace('\\', "/"),
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
