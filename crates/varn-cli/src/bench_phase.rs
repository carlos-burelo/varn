use std::time::Duration;
use varn_utilities::chalk::chalk;
use varn_utilities::terminal;

pub struct PhaseStats {
    pub name: &'static str,
    pub color_fn: fn(varn_utilities::chalk::Chalk) -> varn_utilities::chalk::Chalk,
    pub min: Duration,
    pub p50: Duration,
    pub max: Duration,
    pub total: Duration,
    pub stddev: Duration,
    pub runs: usize,
}

impl PhaseStats {
    pub fn from_samples(
        name: &'static str,
        color_fn: fn(varn_utilities::chalk::Chalk) -> varn_utilities::chalk::Chalk,
        samples: &[Duration],
    ) -> Self {
        let min = *samples.iter().min().expect("samples must not be empty");
        let max = *samples.iter().max().expect("samples must not be empty");
        let total: Duration = samples.iter().sum();
        let mean_ns = total.as_nanos() as f64 / samples.len() as f64;

        let mut sorted = samples.to_vec();
        sorted.sort();
        let p50 = sorted[sorted.len() / 2];

        let variance = samples
            .iter()
            .map(|s| {
                let d = s.as_nanos() as f64 - mean_ns;
                d * d
            })
            .sum::<f64>()
            / samples.len() as f64;
        let stddev = Duration::from_nanos(variance.sqrt() as u64);

        PhaseStats {
            name,
            color_fn,
            min,
            p50,
            max,
            total,
            stddev,
            runs: samples.len(),
        }
    }

    pub fn mean(&self) -> Duration {
        self.total / self.runs as u32
    }
}

pub fn fmt_dur(d: Duration) -> String {
    let ns = d.as_nanos();
    if ns < 1_000 {
        format!("{ns} ns")
    } else if ns < 1_000_000 {
        let us = ns as f64 / 1_000.0;
        if us < 10.0 {
            fmt_f(us, 2) + " µs"
        } else if us < 100.0 {
            fmt_f(us, 1) + " µs"
        } else {
            fmt_f(us, 0) + " µs"
        }
    } else if ns < 1_000_000_000 {
        let ms = ns as f64 / 1_000_000.0;
        if ms < 10.0 {
            fmt_f(ms, 3) + " ms"
        } else if ms < 100.0 {
            fmt_f(ms, 2) + " ms"
        } else {
            fmt_f(ms, 1) + " ms"
        }
    } else {
        let s = ns as f64 / 1_000_000_000.0;
        fmt_f(s, 2) + " s"
    }
}

pub fn fmt_f(f: f64, decimals: usize) -> String {
    if decimals == 0 {
        return format!("{f:.0}");
    }
    let s = format!("{f:.decimals$}");
    let s = s.trim_end_matches('0');
    s.trim_end_matches('.').to_owned()
}

pub fn fmt_bytes(n: usize) -> String {
    if n < 1_024 {
        format!("{n} B")
    } else if n < 1_048_576 {
        format!("{:.1} KB", n as f64 / 1_024.0)
    } else {
        format!("{:.2} MB", n as f64 / 1_048_576.0)
    }
}

pub fn fmt_num(n: usize) -> String {
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

const W_NAME: usize = 10;
const W_TIME: usize = 9;
const W_SIG: usize = 8;
const W_PCT: usize = 6;
const SEP: &str = "─";

pub fn sep_line() {
    let name_col = SEP.repeat(W_NAME);
    let time_col = SEP.repeat(W_TIME);
    let sig_col = SEP.repeat(W_SIG);
    let pct_col = SEP.repeat(W_PCT);
    terminal::log(format!(
        "  {}",
        chalk(format!(
            "{name_col}  {time_col}  {time_col}  {time_col}  {time_col}  {sig_col}  {time_col}  {pct_col}"
        ))
        .dim()
    ));
}

pub fn header_line() {
    terminal::log(format!(
        "  {}",
        chalk(format!(
            "{:<W_NAME$}  {:>W_TIME$}  {:>W_TIME$}  {:>W_TIME$}  {:>W_TIME$}  {:>W_SIG$}  {:>W_TIME$}  {:>W_PCT$}",
            "Phase", "min", "p50", "mean", "max", "σ", "total", "%"
        ))
        .dim()
    ));
}

pub fn phase_line(stat: &PhaseStats, share: f64) {
    let pct = format!("{:.1}%", share * 100.0);
    let name = (stat.color_fn)(chalk(stat.name)).bold();

    terminal::log(format!(
        "  {name:<W_NAME$}  {:>W_TIME$}  {:>W_TIME$}  {}  {:>W_TIME$}  {}  {}  {}",
        fmt_dur(stat.min),
        fmt_dur(stat.p50),
        chalk(format!("{:>W_TIME$}", fmt_dur(stat.mean()))).cyan(),
        fmt_dur(stat.max),
        chalk(format!("{:>W_SIG$}", fmt_dur(stat.stddev))).dim(),
        chalk(format!("{:>W_TIME$}", fmt_dur(stat.total))).dim(),
        chalk(format!("{:>W_PCT$}", pct)).dim(),
    ));
}

pub fn total_line(phases: &[PhaseStats]) {
    let min: Duration = phases.iter().map(|p| p.min).sum();
    let max: Duration = phases.iter().map(|p| p.max).sum();
    let total: Duration = phases.iter().map(|p| p.total).sum();
    let p50: Duration = phases.iter().map(|p| p.p50).sum();
    let runs = phases[0].runs;
    let mean = total / runs as u32;

    terminal::log(format!(
        "  {}  {:>W_TIME$}  {:>W_TIME$}  {}  {:>W_TIME$}  {:>W_SIG$}  {}  {}",
        chalk(format!("{:<W_NAME$}", "total")).green().bold(),
        fmt_dur(min),
        fmt_dur(p50),
        chalk(format!("{:>W_TIME$}", fmt_dur(mean))).cyan(),
        fmt_dur(max),
        "",
        chalk(format!("{:>W_TIME$}", fmt_dur(total))).dim(),
        chalk(format!("{:>W_PCT$}", "100%")).dim(),
    ));
}
