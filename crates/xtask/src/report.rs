use super::args::Opts;
use super::stats::{RowResult, SampleStats};
use super::term::Term;
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Serialize)]
pub(super) struct JsonReport {
    pub(super) cpu: String,
    pub(super) runtimes: Vec<String>,
    pub(super) runs: usize,
    pub(super) warmup: usize,
    pub(super) startup: HashMap<String, SampleStats>,
    pub(super) benchmarks: Vec<JsonBenchRow>,
}

#[derive(Debug, Serialize)]
pub(super) struct JsonBenchRow {
    pub(super) name: String,
    pub(super) output_ok: bool,
    pub(super) verdict: String,
    pub(super) work_ratio: Option<f64>,
    pub(super) resolved: bool,
    pub(super) rival: Option<String>,
    pub(super) rival_work: Option<f64>,
    pub(super) stats: HashMap<String, SampleStats>,
    pub(super) work_stats: HashMap<String, SampleStats>,
}

pub(super) fn print_header(term: &Term, opts: &Opts, cpu: &str, rt_names: &[String]) {
    if opts.json {
        return;
    }
    println!();
    let box_width = 80_usize;
    println!("  {}", term.gray(&format!("┌{}┐", "─".repeat(box_width))));
    let title_plain = "⚡ VARN BENCHMARK SUITE — Comparative Performance Matrix";
    let pad_title = (box_width - 4).saturating_sub(title_plain.chars().count());
    println!(
        "  {}  {}{}{}",
        term.gray("│"),
        term.bold_cyan(title_plain),
        " ".repeat(pad_title),
        term.gray("│")
    );
    let info_line1 = format!(
        "Host: {}   •   Runs: {} ({} warmup)",
        cpu, opts.runs, opts.warmup
    );
    let pad_info1 = (box_width - 4).saturating_sub(info_line1.chars().count());
    println!(
        "  {}  {}{}{}",
        term.gray("│"),
        term.gray(&info_line1),
        " ".repeat(pad_info1),
        term.gray("│")
    );

    let info_line2 = format!("Runtimes: {}", rt_names.join(", "));
    let pad_info2 = (box_width - 4).saturating_sub(info_line2.chars().count());
    println!(
        "  {}  {}{}{}",
        term.gray("│"),
        term.cyan(&info_line2),
        " ".repeat(pad_info2),
        term.gray("│")
    );

    println!("  {}", term.gray(&format!("└{}┘", "─".repeat(box_width))));
    println!();
}

pub(super) fn print_scoreboard(
    term: &Term,
    opts: &Opts,
    results: &[RowResult],
    startup_stats: &HashMap<String, SampleStats>,
) {
    if opts.json {
        return;
    }
    let mut varn_wins = 0;
    let mut rival_wins = 0;
    let mut tied = 0;
    let mut mismatches = 0;

    for r in results {
        if !r.output_ok {
            mismatches += 1;
        } else if !r.resolved {
            tied += 1;
        } else if let Some(ratio) = r.work_ratio {
            if ratio >= 1.05 {
                varn_wins += 1;
            } else if ratio <= 0.95 {
                rival_wins += 1;
            } else {
                tied += 1;
            }
        } else {
            tied += 1;
        }
    }

    let su_speedup =
        if let (Some(v), Some(b)) = (startup_stats.get("varn"), startup_stats.get("bun")) {
            b.median / v.median.max(0.1)
        } else {
            1.0
        };

    let box_width = 80_usize;
    println!("  {}", term.gray(&format!("┌{}┐", "─".repeat(box_width))));
    let title_line = format!(
        "📊 SCOREBOARD:   🏆 {} Wins   •   🤝 {} Tied   •   🔻 {} Rivals",
        varn_wins, tied, rival_wins
    );
    let title_colored = format!(
        "{}   {} {}   •   {} {}   •   {} {}",
        term.bold_white("📊 SCOREBOARD:"),
        "🏆",
        term.bold_green(&format!("{varn_wins} Wins")),
        "🤝",
        term.cyan(&format!("{tied} Tied")),
        "🔻",
        term.yellow(&format!("{rival_wins} Rivals"))
    );
    let pad_title = (box_width - 4).saturating_sub(title_line.chars().count());
    println!(
        "  {}  {}{}  {}",
        term.gray("│"),
        title_colored,
        " ".repeat(pad_title),
        term.gray("│")
    );

    let integrity = if mismatches == 0 {
        term.bold_green("100% Verified (Zero mismatches)")
    } else {
        term.bold_red(&format!("{mismatches} MISMATCHES"))
    };
    let sub_colored = format!(
        "🚀 Startup: {} faster than Bun   •   Integrity: {}",
        term.bold_green(&format!("{:.1}x", su_speedup)),
        integrity
    );
    let sub_plain = format!(
        "🚀 Startup: {:.1}x faster than Bun   •   Integrity: {}",
        su_speedup,
        if mismatches == 0 {
            "100% Verified (Zero mismatches)"
        } else {
            "MISMATCHES"
        }
    );
    let pad_sub = (box_width - 4).saturating_sub(sub_plain.chars().count());
    println!(
        "  {}  {}{}  {}",
        term.gray("│"),
        sub_colored,
        " ".repeat(pad_sub),
        term.gray("│")
    );
    println!("  {}", term.gray(&format!("└{}┘", "─".repeat(box_width))));
    println!();
}
