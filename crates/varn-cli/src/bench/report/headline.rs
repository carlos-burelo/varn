//! The verdict block: visual dashboard shown before verbose sections.

use std::time::Duration;

use varn_core::term::chalk::chalk;
use varn_core::term::terminal;
use varn_vm::varn_jit::JitStatsSnapshot;

use super::fmt::{fmt_bytes, fmt_dur, fmt_num, fmt_pct, short_path};
use crate::bench::stats::{sparkline, PhaseStats, CV_UNRELIABLE};

/// Inner content width of the dashboard box (chars between `│ ` and ` │`).
const INNER: usize = 64;
/// Width of the phase proportional bars (frontend / execute rows).
const BAR_W: usize = 30;
/// Width of the JIT coverage bar.
const JIT_BAR_W: usize = 34;

// ─── Box rendering helpers ────────────────────────────────────────────────

/// Count visible chars in a string, skipping ANSI SGR sequences (`\x1b[...m`).
fn visible_len(s: &str) -> usize {
    let mut n = 0;
    let mut in_esc = false;
    for c in s.chars() {
        match (in_esc, c) {
            (_, '\x1b') => in_esc = true,
            (true, 'm') => in_esc = false,
            (true, _) => {}
            (false, _) => n += 1,
        }
    }
    n
}

/// A content line inside the box, padded to INNER visible chars.
fn box_line(content: &str) -> String {
    let vis = visible_len(content);
    let pad = INNER.saturating_sub(vis);
    format!("  │ {}{} │", content, " ".repeat(pad))
}

/// Horizontal rule separating sections inside the box.
fn box_rule() -> String {
    format!("  ├{}┤", "─".repeat(INNER + 2))
}

/// Box bottom border.
fn box_bottom() -> String {
    format!("  ╰{}╯", "─".repeat(INNER + 2))
}

// ─── Bar helpers ─────────────────────────────────────────────────────────

/// Proportional bar with 1/8-block sub-pixel precision.
/// Returns exactly `width` visible chars.
fn proportional_bar(share: f64, width: usize) -> String {
    const EIGHTHS: [char; 7] = ['▏', '▎', '▍', '▌', '▋', '▊', '▉'];
    let share = share.clamp(0.0, 1.0);
    if width == 0 {
        return String::new();
    }
    let total = ((share * (width * 8) as f64).round() as usize).min(width * 8);
    let full = (total / 8).min(width);
    let frac = total % 8;
    let mut s = String::with_capacity(width * 3);
    for _ in 0..full {
        s.push('█');
    }
    if full < width {
        s.push(if frac > 0 { EIGHTHS[frac - 1] } else { ' ' });
        for _ in (full + 1)..width {
            s.push(' ');
        }
    }
    s
}

/// Two-tone bar: `█` for JIT frames, `░` for interpreter.
fn jit_bar(ratio: f64, width: usize) -> String {
    let filled = ((ratio.clamp(0.0, 1.0) * width as f64).round() as usize).min(width);
    (0..width)
        .map(|i| if i < filled { '█' } else { '░' })
        .collect()
}

// ─── Types ────────────────────────────────────────────────────────────────

/// Which build produced these numbers.
pub struct BuildId {
    pub profile: &'static str,
    pub backend: &'static str,
    pub commit: Option<&'static str>,
}

impl BuildId {
    pub fn detect() -> Self {
        let backend = if varn_vm::ExecSettings::from_env(false).no_jit {
            "interp (VARN_NO_JIT)"
        } else if varn_vm::varn_jit::clif::enabled() {
            "clif"
        } else {
            "interp"
        };
        Self {
            profile: if cfg!(debug_assertions) { "debug" } else { "release" },
            backend,
            commit: option_env!("VARN_GIT_SHA"),
        }
    }

    fn jit_disabled(&self) -> bool {
        self.backend.starts_with("interp")
    }

    fn render(&self, runs: usize) -> String {
        let mut parts = vec![
            format!("{runs} runs"),
            self.profile.to_owned(),
            self.backend.to_owned(),
        ];
        if let Some(sha) = self.commit {
            parts.push(sha.to_owned());
        }
        parts.join(" · ")
    }
}

/// The compile-versus-run split of one execution.
pub struct ExecSplit {
    pub compile: Duration,
    #[allow(dead_code)]
    pub run: Option<Duration>,
    #[allow(dead_code)]
    pub functions: u64,
}

impl ExecSplit {
    pub fn from_single_run(execute: Duration, jit: &JitStatsSnapshot) -> Option<Self> {
        if jit.compile_success == 0 {
            return None;
        }
        let compile = Duration::from_nanos(jit.total_compile_time_ns);
        Some(Self {
            run: (compile <= execute).then(|| execute - compile),
            compile,
            functions: jit.compile_success,
        })
    }
}

pub struct Headline<'a> {
    pub path: &'a str,
    pub runs: usize,
    pub source_lines: usize,
    pub source_bytes: u64,
    pub tokens: usize,
    pub e2e: Option<&'a PhaseStats>,
    pub execute: Option<&'a PhaseStats>,
    pub total_p50: Duration,
    pub split: Option<ExecSplit>,
    pub jit: Option<&'a JitStatsSnapshot>,
    #[allow(dead_code)]
    pub coverage_scope: &'a str,
    pub top_blocker: Option<(String, String)>,
    pub cpu: Option<crate::cpu_freq::CpuFreq>,
    /// All pipeline phases — used to render proportional bars.
    pub phases: Option<&'a [PhaseStats]>,
    /// Raw e2e duration samples — used to render the sparkline.
    pub e2e_samples: Option<&'a [Duration]>,
}

impl Headline<'_> {
    pub fn print(&self) {
        let build = BuildId::detect();
        terminal::blank();

        // ── Box top ──────────────────────────────────────────────────────
        let left = format!("bench · {}", short_path(self.path));
        let right = build.render(self.runs);
        // Truncate path so it always fits.
        let max_left = INNER.saturating_sub(right.chars().count() + 6);
        let left = if left.chars().count() > max_left {
            super::fmt::truncate_middle(&left, max_left)
        } else {
            left
        };
        let dashes = INNER.saturating_sub(4 + left.chars().count() + right.chars().count());
        let top = format!(
            "  ╭─ {} {} {} ─╮",
            chalk(&left).bold(),
            "─".repeat(dashes),
            chalk(&right).dim(),
        );
        terminal::log(top);

        // ── Stats line ───────────────────────────────────────────────────
        let e2e_dur = self.e2e.map(|e| e.p50).unwrap_or(self.total_p50);
        let throughput = 1_000_000_000.0 / e2e_dur.as_nanos().max(1) as f64;
        let size_part = if self.source_lines > 0 {
            format!(
                "  ·  {} L · {} tokens",
                fmt_num(self.source_lines as u64),
                fmt_num(self.tokens as u64)
            )
        } else {
            format!("  ·  {}", fmt_bytes(self.source_bytes))
        };
        let stats = format!(
            "{}  p50  ·  {:.1}/s{}",
            chalk(fmt_dur(e2e_dur)).bold(),
            throughput,
            chalk(size_part).dim(),
        );
        terminal::log(box_line(&stats));

        // ── Phase bars ───────────────────────────────────────────────────
        if let Some(phases) = self.phases {
            terminal::log(box_rule());
            let total_ns = self.total_p50.as_nanos() as f64;

            let frontend_p50: Duration =
                phases.iter().filter(|p| p.name != "execute").map(|p| p.p50).sum();
            let execute_p50 = phases
                .iter()
                .find(|p| p.name == "execute")
                .map(|p| p.p50)
                .unwrap_or(Duration::ZERO);

            let fe_share =
                if total_ns > 0.0 { frontend_p50.as_nanos() as f64 / total_ns } else { 0.0 };
            let ex_share =
                if total_ns > 0.0 { execute_p50.as_nanos() as f64 / total_ns } else { 0.0 };

            // name(10) + bar(BAR_W) + "  " + dur(9) + "  " + pct(5) = visible
            let fe_name = format!("{:<10}", "frontend");
            let fe_bar = chalk(proportional_bar(fe_share, BAR_W)).dim().to_string();
            let fe_dur = format!("{:<9}", fmt_dur(frontend_p50));
            let fe_pct = format!("{:>5}", fmt_pct(fe_share));
            let fe_line =
                format!("{}{}  {}  {}", chalk(fe_name).dim(), fe_bar, chalk(fe_dur).dim(), fe_pct);
            terminal::log(box_line(&fe_line));

            let ex_name = format!("{:<10}", "execute");
            let ex_bar = chalk(proportional_bar(ex_share, BAR_W)).blue().to_string();
            let ex_dur = format!("{:<9}", fmt_dur(execute_p50));
            let ex_pct = format!("{:>5}", fmt_pct(ex_share));
            let compile_note = self.split.as_ref().map(|s| {
                chalk(format!("  [{} compile]", fmt_dur(s.compile))).dim().to_string()
            }).unwrap_or_default();
            let ex_line = format!(
                "{}{}  {}  {}{}",
                chalk(ex_name).blue().bold(),
                ex_bar,
                chalk(ex_dur).blue(),
                chalk(ex_pct).dim(),
                compile_note,
            );
            terminal::log(box_line(&ex_line));
        }

        // ── JIT bar + distribution ────────────────────────────────────────
        let show_jit_section =
            (self.jit.is_some() && !build.jit_disabled()) || self.e2e_samples.is_some();
        if show_jit_section {
            terminal::log(box_rule());
        }

        if !build.jit_disabled() {
            if let Some(jit) = self.jit {
                let ratio = jit.machine_code_ratio();
                let machine = jit.machine_code_frames();
                let total = jit.total_frames();

                let bar_str = jit_bar(ratio, JIT_BAR_W);
                let bar_colored = if ratio >= 0.8 {
                    chalk(&bar_str).green().to_string()
                } else if ratio >= 0.3 {
                    chalk(&bar_str).yellow().to_string()
                } else {
                    chalk(&bar_str).red().to_string()
                };
                let pct_colored = if ratio >= 0.8 {
                    chalk(fmt_pct(ratio)).green().bold().to_string()
                } else {
                    chalk(fmt_pct(ratio)).yellow().bold().to_string()
                };
                let counts = chalk(format!("({machine}/{total})")).dim().to_string();

                // "JIT  " + bar(JIT_BAR_W) + "  " + pct(6) + "  " + counts(~10)
                let jit_line = format!("JIT  {}  {}  {}", bar_colored, pct_colored, counts);
                terminal::log(box_line(&jit_line));

                // Blockers / notes (compact, not the full verbose section)
                let never = jit.never_compiled_frames();
                if never > 0 && jit.functions_seen() > 0 {
                    let note = if let Some((name, reason)) = &self.top_blocker {
                        format!(
                            "{} frames interpreted  ·  {}",
                            fmt_num(never),
                            super::fmt::truncate_middle(&format!("{name}: {reason}"), 36)
                        )
                    } else {
                        format!("{} frames interpreted", fmt_num(never))
                    };
                    terminal::log(box_line(&chalk(note).dim().to_string()));
                }
            } else if self.phases.is_some() || self.e2e_samples.is_some() {
                // JIT enabled at build but no stats collected — silent skip.
            }
        } else if show_jit_section {
            terminal::log(box_line(
                &chalk("JIT desactivado — línea base de intérprete").dim().to_string(),
            ));
        }

        // ── Sparkline + distribution ──────────────────────────────────────
        if let Some(e2e) = self.e2e {
            if let Some(samples) = self.e2e_samples {
                let spark = sparkline(samples);
                let spread = e2e.spread();
                let spread_warn = self.execute.map(|ex| ex.cv() >= CV_UNRELIABLE).unwrap_or(false);

                let dist = format!(
                    "{} – {}  ·  σ {}",
                    fmt_dur(e2e.min),
                    fmt_dur(e2e.max),
                    fmt_pct(e2e.cv()),
                );
                let spread_part = if spread_warn {
                    format!("  {}", chalk(format!("⚠ spread {}", fmt_pct(spread))).yellow())
                } else {
                    String::new()
                };

                // Throttle note if CPU is significantly collapsed.
                let throttle = self.cpu.as_ref().and_then(|cf| {
                    if cf.max_mhz > 0 && cf.cur_mhz as f64 / (cf.max_mhz as f64) < 0.90 {
                        Some(format!(
                            "  ·  CPU {} MHz ({})",
                            cf.cur_mhz,
                            fmt_pct(cf.cur_mhz as f64 / cf.max_mhz as f64)
                        ))
                    } else {
                        None
                    }
                });

                let dist_line = format!(
                    "{}  {}{}{}",
                    chalk(spark).dim(),
                    chalk(dist).dim(),
                    spread_part,
                    throttle.as_deref().map(|t| chalk(t).dim().to_string()).unwrap_or_default(),
                );
                terminal::log(box_line(&dist_line));
            }
        }

        terminal::log(box_bottom());
    }
}
