use std::time::Duration;

use varn_core::term::chalk::chalk;
use varn_core::term::terminal;
use varn_jit::JitStatsSnapshot;

use super::fmt::{fmt_bytes, fmt_dur, fmt_num, fmt_pct, short_path};
use crate::bench::stats::{PhaseStats, CV_UNRELIABLE};

pub struct BuildId {
    pub profile: &'static str,
    pub backend: &'static str,
    pub commit: Option<&'static str>,
}

impl BuildId {
    pub fn detect() -> Self {
        let backend = if varn_vm::ExecSettings::from_env(false).no_jit {
            "interp (VARN_NO_JIT)"
        } else if varn_jit::clif::enabled() {
            "clif"
        } else {
            "interp"
        };
        Self {
            profile: if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            },
            backend,
            commit: option_env!("VARN_GIT_SHA"),
        }
    }

    fn jit_disabled(&self) -> bool {
        self.backend.starts_with("interp")
    }
}

pub struct ExecSplit {
    pub compile: Duration,
}

impl ExecSplit {
    pub fn from_single_run(execute: Duration, jit: &JitStatsSnapshot) -> Option<Self> {
        if jit.compile_success == 0 {
            return None;
        }
        let compile = Duration::from_nanos(jit.total_compile_time_ns);
        (compile <= execute).then_some(Self { compile })
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
    pub top_blocker: Option<(String, String)>,
    pub tiered_during_window: u64,
    pub cpu: Option<crate::cpu_freq::CpuFreq>,
    pub phases: Option<&'a [PhaseStats]>,
}

impl Headline<'_> {
    pub fn print(&self) {
        use terminal::Align::{Left, Right};
        let build = BuildId::detect();

        terminal::Section::new(format!("bench · {}", short_path(self.path)))
            .subtitle(format!(
                "{} runs · {} · {}",
                self.runs, build.profile, build.backend
            ))
            .print();

        let mut t = terminal::Table::new(["metric", "value"]).align([Left, Right]);
        t.row(["runs".to_owned(), fmt_num(self.runs as u64)]);
        t.row(["profile".to_owned(), build.profile.to_owned()]);
        t.row(["backend".to_owned(), build.backend.to_owned()]);
        t.row(["commit".to_owned(), build.commit.unwrap_or("—").to_owned()]);
        t.print();

        let e2e_dur = self.e2e.map(|e| e.p50).unwrap_or(self.total_p50);
        let throughput = 1_000_000_000.0 / e2e_dur.as_nanos().max(1) as f64;
        terminal::Section::new("result").print();
        let mut t = terminal::Table::new(["metric", "value"]).align([Left, Right]);
        t.row(["e2e p50".to_owned(), fmt_dur(e2e_dur)]);
        t.row([("throughput".to_owned()), format!("{throughput:.1}/s")]);
        if self.source_lines > 0 {
            t.row(["source lines".to_owned(), fmt_num(self.source_lines as u64)]);
            t.row(["tokens".to_owned(), fmt_num(self.tokens as u64)]);
        }
        t.row(["source bytes".to_owned(), fmt_bytes(self.source_bytes)]);
        t.print();

        if let Some(phases) = self.phases {
            terminal::Section::new("phases")
                .subtitle("p50 + share")
                .print();
            let total_ns = self.total_p50.as_nanos() as f64;
            let mut t = terminal::Table::new(["phase", "p50", "share"]).align([Left, Right, Right]);
            for phase in phases {
                let share = if total_ns > 0.0 {
                    phase.p50.as_nanos() as f64 / total_ns
                } else {
                    0.0
                };
                let mut name = phase.name.to_owned();
                if phase.name == "execute (warm)" {
                    if let Some(s) = &self.split {
                        name = format!("{} (compile {})", name, fmt_dur(s.compile));
                    }
                }
                t.row([
                    (phase.color_fn)(chalk(name.as_str())).bold().to_string(),
                    fmt_dur(phase.p50),
                    fmt_pct(share),
                ]);
            }
            t.print();
        }

        terminal::Section::new("jit").print();
        if build.jit_disabled() {
            terminal::log(format!(
                "  {}",
                chalk("JIT desactivado — línea base de intérprete").dim()
            ));
        } else if let Some(jit) = self.jit {
            let ratio = jit.fn_compilation_rate();
            let compiled = jit.compile_success;
            let total_fns = jit.functions_seen();
            let blocked = jit.gate_rejected + jit.compile_fail;
            let mut t = terminal::Table::new(["metric", "value"]).align([Left, Right]);
            t.row([
                "compiled".to_owned(),
                format!("{}/{} fns", fmt_num(compiled), fmt_num(total_fns)),
            ]);
            t.row(["ratio".to_owned(), fmt_pct(ratio)]);
            t.row(["blocked".to_owned(), fmt_num(blocked)]);
            t.row([
                "top blocker".to_owned(),
                self.top_blocker
                    .as_ref()
                    .map(|(n, r)| super::fmt::truncate_middle(&format!("{n}: {r}"), 60))
                    .unwrap_or_else(|| "—".to_owned()),
            ]);
            t.row([
                "warmup frames (tiering)".to_owned(),
                fmt_num(jit.never_compiled_frames()),
            ]);
            t.row([
                "compiled during window".to_owned(),
                fmt_num(self.tiered_during_window),
            ]);
            t.print();
        } else {
            terminal::log(format!("  {}", chalk("sin datos JIT").dim()));
        }

        if let Some(e2e) = self.e2e {
            terminal::Section::new("distribution").print();
            let mut t = terminal::Table::new(["metric", "value"]).align([Left, Right]);
            t.row(["min".to_owned(), fmt_dur(e2e.min)]);
            t.row(["max".to_owned(), fmt_dur(e2e.max)]);
            t.row(["cv".to_owned(), fmt_pct(e2e.cv())]);
            t.row(["spread".to_owned(), fmt_pct(e2e.spread())]);
            t.print();
            let spread_warn = self
                .execute
                .map(|ex| ex.cv() >= CV_UNRELIABLE)
                .unwrap_or(false);
            if spread_warn {
                terminal::warn(format!("spread {}", fmt_pct(e2e.spread())));
            }
            if let Some(cf) = &self.cpu {
                if cf.max_mhz > 0 && cf.cur_mhz as f64 / (cf.max_mhz as f64) < 0.90 {
                    terminal::warn(format!(
                        "throttle CPU {}MHz ({})",
                        cf.cur_mhz,
                        fmt_pct(cf.cur_mhz as f64 / (cf.max_mhz as f64))
                    ));
                }
            }
        }
    }
}
