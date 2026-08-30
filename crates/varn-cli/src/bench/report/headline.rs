//! The verdict block: visual dashboard shown before verbose sections.

use std::time::Duration;

use varn_core::term::chalk::chalk;
use varn_core::term::terminal;
use varn_vm::varn_jit::JitStatsSnapshot;

use super::fmt::{fmt_bytes, fmt_dur, fmt_num, fmt_pct, short_path};
use crate::bench::stats::{freq_histogram, PhaseStats, CV_UNRELIABLE};

/// Inner content width of the dashboard box (visible chars between `│ ` and ` │`).
const INNER: usize = 66;
/// Width of the individual phase proportional bars.
const BAR_W: usize = 26;
/// Width of the JIT coverage bar.
const JIT_BAR_W: usize = 36;
/// Histogram columns shown in the distribution row.
const HIST_COLS: usize = 16;

// ─── Box rendering ────────────────────────────────────────────────────────

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

fn box_line(content: &str) -> String {
    let vis = visible_len(content);
    let pad = INNER.saturating_sub(vis);
    format!("  │ {}{} │", content, " ".repeat(pad))
}

fn box_rule() -> String {
    format!("  ├{}┤", "─".repeat(INNER + 2))
}

fn box_bottom() -> String {
    format!("  ╰{}╯", "─".repeat(INNER + 2))
}

// ─── Bar helpers ─────────────────────────────────────────────────────────

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

fn jit_bar(ratio: f64, width: usize) -> String {
    let filled = ((ratio.clamp(0.0, 1.0) * width as f64).round() as usize).min(width);
    (0..width)
        .map(|i| if i < filled { '█' } else { '░' })
        .collect()
}

fn jit_grade(ratio: f64) -> String {
    if ratio >= 0.90 {
        chalk("[A]").green().bold().to_string()
    } else if ratio >= 0.70 {
        chalk("[B]").cyan().bold().to_string()
    } else if ratio >= 0.50 {
        chalk("[C]").yellow().bold().to_string()
    } else if ratio >= 0.30 {
        chalk("[D]").yellow().to_string()
    } else {
        chalk("[F]").red().bold().to_string()
    }
}

/// Compact duration without trailing decimals (used for tight inline notes).
fn fmt_compact(d: Duration) -> String {
    let ns = d.as_nanos();
    if ns < 1_000 {
        format!("{ns}ns")
    } else if ns < 1_000_000 {
        format!("{}µs", ns / 1_000)
    } else {
        format!("{:.1}ms", ns as f64 / 1_000_000.0)
    }
}

// ─── Types ────────────────────────────────────────────────────────────────

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
    /// All pipeline phases — renders individual proportional bars.
    pub phases: Option<&'a [PhaseStats]>,
    /// Raw e2e duration samples — renders the frequency histogram.
    pub e2e_samples: Option<&'a [Duration]>,
}

impl Headline<'_> {
    pub fn print(&self) {
        let build = BuildId::detect();
        terminal::blank();

        // ── Box top ──────────────────────────────────────────────────────
        let left = format!("bench · {}", short_path(self.path));
        let right = build.render(self.runs);
        let max_left = INNER.saturating_sub(right.chars().count() + 6);
        let left = if left.chars().count() > max_left {
            super::fmt::truncate_middle(&left, max_left)
        } else {
            left
        };
        let dashes = INNER.saturating_sub(4 + left.chars().count() + right.chars().count());
        terminal::log(format!(
            "  ╭─ {} {} {} ─╮",
            chalk(&left).bold(),
            "─".repeat(dashes),
            chalk(&right).dim(),
        ));

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

        // ── Individual phase bars ─────────────────────────────────────────
        if let Some(phases) = self.phases {
            terminal::log(box_rule());
            let total_ns = self.total_p50.as_nanos() as f64;

            for phase in phases {
                let share = if total_ns > 0.0 {
                    phase.p50.as_nanos() as f64 / total_ns
                } else {
                    0.0
                };

                let bar_str = proportional_bar(share, BAR_W);
                let colored_bar = (phase.color_fn)(chalk(bar_str.as_str())).to_string();
                let name_plain = format!("{:<10}", phase.name);
                let colored_name = (phase.color_fn)(chalk(name_plain.as_str()))
                    .bold()
                    .to_string();
                let dur_str = format!("{:<9}", fmt_dur(phase.p50));
                let pct_str = format!("{:>5}", fmt_pct(share));

                // For execute: show JIT compile note inline if it fits.
                let compile_note = if phase.name == "execute" {
                    self.split
                        .as_ref()
                        .map(|s| {
                            chalk(format!("  ↪{}", fmt_compact(s.compile))).dim().to_string()
                        })
                        .unwrap_or_default()
                } else {
                    String::new()
                };

                let colored_dur = (phase.color_fn)(chalk(dur_str.as_str())).to_string();
                let row = format!(
                    "{}{}  {}  {}{}",
                    colored_name, colored_bar, colored_dur, pct_str, compile_note
                );
                terminal::log(box_line(&row));
            }
        }

        // ── JIT bar + grade ───────────────────────────────────────────────
        let show_jit_section =
            (self.jit.is_some() && !build.jit_disabled()) || self.e2e_samples.is_some();
        if show_jit_section {
            terminal::log(box_rule());
        }

        if !build.jit_disabled() {
            if let Some(jit) = self.jit {
                // fn_compilation_rate is the only accurate metric: counts each
                // unique function once, unaffected by JIT-direct recursive calls
                // that bypass the interpreter trampoline entirely.
                let ratio = jit.fn_compilation_rate();
                let compiled = jit.compile_success;
                let total_fns = jit.functions_seen();

                let bar_str = jit_bar(ratio, JIT_BAR_W);
                let bar_colored = if ratio >= 0.90 {
                    chalk(&bar_str).green().to_string()
                } else if ratio >= 0.50 {
                    chalk(&bar_str).yellow().to_string()
                } else {
                    chalk(&bar_str).red().to_string()
                };
                let pct_colored = if ratio >= 0.90 {
                    chalk(fmt_pct(ratio)).green().bold().to_string()
                } else if ratio >= 0.70 {
                    chalk(fmt_pct(ratio)).cyan().bold().to_string()
                } else {
                    chalk(fmt_pct(ratio)).yellow().bold().to_string()
                };
                let grade = jit_grade(ratio);
                let counts = chalk(format!("({compiled}/{total_fns} fns)")).dim().to_string();

                let jit_line =
                    format!("JIT  {}  {} {}  {}", bar_colored, pct_colored, grade, counts);
                terminal::log(box_line(&jit_line));

                // Note: distinguish uncompilable functions (real problem) from
                // tiering warm-up frames (expected behavior, not a defect).
                let blocked = jit.gate_rejected + jit.compile_fail;
                if blocked > 0 {
                    let note = if let Some((name, reason)) = &self.top_blocker {
                        let reason_short =
                            super::fmt::truncate_middle(&format!("{name}: {reason}"), 40);
                        format!(
                            "{} fns sin compilar  ·  {}",
                            fmt_num(blocked),
                            reason_short
                        )
                    } else {
                        format!("{} fns sin compilar", fmt_num(blocked))
                    };
                    terminal::log(box_line(&chalk(note).dim().to_string()));
                } else if jit.never_compiled_frames() > 0 {
                    let tiering = jit.never_compiled_frames();
                    let note = format!(
                        "{} entradas de calentamiento (tiering)",
                        fmt_num(tiering)
                    );
                    terminal::log(box_line(&chalk(note).dim().to_string()));
                }
            }
        } else if show_jit_section {
            terminal::log(box_line(
                &chalk("JIT desactivado — línea base de intérprete").dim().to_string(),
            ));
        }

        // ── Frequency histogram + distribution ────────────────────────────
        if let Some(e2e) = self.e2e {
            if let Some(samples) = self.e2e_samples {
                let hist = freq_histogram(samples, HIST_COLS);
                let spread = e2e.spread();
                let spread_warn = self.execute.map(|ex| ex.cv() >= CV_UNRELIABLE).unwrap_or(false);

                let dist = format!(
                    "{}  {}  σ {}  {}–{}",
                    chalk(hist).dim(),
                    chalk(format!("{:.1}/s", 1e9 / e2e.p50.as_nanos().max(1) as f64)).dim(),
                    chalk(fmt_pct(e2e.cv())).dim(),
                    fmt_dur(e2e.min),
                    fmt_dur(e2e.max),
                );
                let spread_part = if spread_warn {
                    format!("  {}", chalk(format!("⚠ spread {}", fmt_pct(spread))).yellow())
                } else {
                    String::new()
                };

                // Throttle note.
                let throttle = self.cpu.as_ref().and_then(|cf| {
                    if cf.max_mhz > 0 && cf.cur_mhz as f64 / (cf.max_mhz as f64) < 0.90 {
                        Some(
                            chalk(format!(
                                "  CPU {}MHz ({})",
                                cf.cur_mhz,
                                fmt_pct(cf.cur_mhz as f64 / (cf.max_mhz as f64))
                            ))
                            .dim()
                            .to_string(),
                        )
                    } else {
                        None
                    }
                });

                let dist_line = format!(
                    "{}{}{}",
                    dist,
                    spread_part,
                    throttle.unwrap_or_default(),
                );
                terminal::log(box_line(&dist_line));
            }
        }

        terminal::log(box_bottom());
    }
}
