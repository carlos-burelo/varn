//! The verdict block: what this run says, before any table.

use std::time::Duration;

use varn_utilities::chalk::chalk;
use varn_utilities::terminal;
use varn_vm::varn_jit::JitStatsSnapshot;

use super::fmt::{fmt_bytes, fmt_dur, fmt_num, fmt_pct, short_path};
use crate::bench::stats::{PhaseStats, CV_UNRELIABLE};

/// Which build produced these numbers. Without it a saved bench result cannot
/// be compared against another one months later.
pub struct BuildId {
    pub profile: &'static str,
    pub backend: &'static str,
    pub commit: Option<&'static str>,
}

impl BuildId {
    pub fn detect() -> Self {
        // `VARN_NO_JIT` closes the door into compiled code at runtime and is
        // independent of whether the Cranelift backend was built in. Reporting
        // `clif` off `clif::enabled()` alone labelled an interpreter-only run
        // as clif — the one mislabel that makes a saved benchmark actively
        // misleading.
        let backend = if varn_vm::ExecSettings::from_env(false).no_jit {
            "interp (VARN_NO_JIT)"
        } else if varn_vm::varn_jit::clif::enabled() {
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
            // Defined by the build script when git metadata is available; the
            // field is omitted rather than faked when it is not.
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
    /// Lowering time summed over every thread that compiled.
    pub compile: Duration,
    /// Execute time left after compilation. `None` when the split cannot be
    /// stated as a subtraction — see [`ExecSplit::concurrent`].
    pub run: Option<Duration>,
    pub functions: u64,
}

impl ExecSplit {
    /// `jit` must describe a *single* execution. Averaging a multi-run
    /// snapshot mixes per-run and cumulative quantities, which is how the
    /// first cut of this report produced "compilar 144% del execute".
    pub fn from_single_run(execute: Duration, jit: &JitStatsSnapshot) -> Option<Self> {
        if jit.compile_success == 0 {
            return None;
        }
        let compile = Duration::from_nanos(jit.total_compile_time_ns);
        Some(Self {
            // `None` when aggregate compile time exceeds the wall-clock
            // execute phase: threads compiled in parallel (isolates), so the
            // two numbers live on different clocks and subtracting one from
            // the other is meaningless.
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
    /// Label for what the coverage figures cover.
    pub coverage_scope: &'a str,
    pub top_blocker: Option<(String, String)>,
    pub cpu: Option<crate::cpu_freq::CpuFreq>,
}

/// Only worth printing when the clock actually collapsed: a CPU sitting at or
/// above its rated clock explains nothing about a noisy measurement, and the
/// old output repeated "base" three times to say exactly that.
fn throttle_note(cf: &crate::cpu_freq::CpuFreq) -> Option<String> {
    if cf.max_mhz == 0 {
        return None;
    }
    let pct = cf.cur_mhz as f64 / cf.max_mhz as f64;
    (pct < 0.90).then(|| {
        format!(
            "CPU a {} MHz, {} de su reloj base ({} MHz) — throttling",
            cf.cur_mhz,
            fmt_pct(pct),
            cf.max_mhz
        )
    })
}

impl Headline<'_> {
    pub fn print(&self) {
        let build = BuildId::detect();

        terminal::blank();
        terminal::log(format!(
            "  {} · {}",
            chalk("varn bench").cyan().bold(),
            chalk(short_path(self.path)).bold()
        ));
        terminal::log(format!("  {}", chalk(build.render(self.runs)).dim()));
        terminal::log(format!(
            "  {}",
            chalk(format!(
                "{} líneas · {} · {} tokens",
                fmt_num(self.source_lines as u64),
                fmt_bytes(self.source_bytes),
                fmt_num(self.tokens as u64)
            ))
            .dim()
        ));
        terminal::blank();

        let headline_dur = self.e2e.map(|e| e.p50).unwrap_or(self.total_p50);
        let throughput = if headline_dur.as_nanos() > 0 {
            1_000_000_000.0 / headline_dur.as_nanos() as f64
        } else {
            f64::INFINITY
        };
        terminal::log(format!(
            "  {}  {}     {}",
            chalk(fmt_dur(headline_dur)).bold(),
            chalk("p50 e2e").dim(),
            chalk(format!("{throughput:.1} runs/s")).dim()
        ));

        if let (Some(exec), Some(split)) = (self.execute, self.split.as_ref()) {
            terminal::blank();
            let total_ns = exec.p50.as_nanos() as f64;
            let cshare = if total_ns > 0.0 {
                split.compile.as_nanos() as f64 / total_ns
            } else {
                0.0
            };

            match split.run {
                Some(run) => terminal::log(format!(
                    "  {} {}  =  {} ({})  +  {} ({})",
                    chalk("execute").blue().bold(),
                    fmt_dur(exec.p50),
                    chalk(format!("compilar {}", fmt_dur(split.compile))).yellow(),
                    fmt_pct(cshare),
                    chalk(format!("correr {}", fmt_dur(run))).green(),
                    fmt_pct(1.0 - cshare),
                )),
                None => {
                    terminal::log(format!(
                        "  {} {}   {} {}",
                        chalk("execute").blue().bold(),
                        fmt_dur(exec.p50),
                        chalk("compilar").yellow(),
                        chalk(format!("{} agregado", fmt_dur(split.compile))).yellow(),
                    ));
                    terminal::log(format!(
                        "  {}",
                        chalk(
                            "compilación concurrente en isolates: el agregado excede el \
                             wall-clock, no es restable"
                        )
                        .dim()
                    ));
                }
            }

            let per_fn = if split.functions > 0 {
                split.compile / split.functions as u32
            } else {
                Duration::ZERO
            };
            terminal::log(format!(
                "  {}",
                chalk(format!(
                    "{} fns compiladas por corrida · {}/fn  ← sin amortizar entre corridas",
                    fmt_num(split.functions),
                    fmt_dur(per_fn),
                ))
                .dim()
            ));
        }

        if build.jit_disabled() {
            terminal::blank();
            terminal::log(format!(
                "  {}",
                chalk("JIT desactivado — todo corre por el intérprete; línea base de paridad, no de rendimiento")
                    .dim()
            ));
        } else if let Some(jit) = self.jit {
            terminal::blank();
            let ratio = jit.clif_frame_ratio();
            let colour = if jit.interp_runs == 0 {
                chalk(fmt_pct(ratio)).green()
            } else {
                chalk(fmt_pct(ratio)).yellow()
            };
            terminal::log(format!(
                "  {} {} de {} frames ({})  {}",
                chalk("cobertura clif").bold(),
                fmt_num(jit.clif_frames()),
                fmt_num(jit.total_frames()),
                colour,
                chalk(format!("[ámbito: {}]", self.coverage_scope)).dim()
            ));

            let off_clif = jit.compile_fail + jit.gate_rejected;
            if off_clif > 0 {
                terminal::log(format!(
                    "  {} {}",
                    chalk("✗").red(),
                    chalk(format!(
                        "{} de {} funciones ofrecidas al JIT quedaron fuera",
                        fmt_num(off_clif),
                        fmt_num(jit.functions_seen())
                    ))
                    .red()
                ));
                if let Some((name, reason)) = &self.top_blocker {
                    terminal::log(format!(
                        "      {}   {}",
                        chalk(name).bold(),
                        chalk(reason).dim()
                    ));
                }
            }
            if jit.interp_runs > 0 {
                terminal::log(format!(
                    "  {} {}",
                    chalk("✗").red(),
                    chalk(format!("{} frames al intérprete", fmt_num(jit.interp_runs))).red()
                ));
                if off_clif == 0 {
                    // Every function the JIT saw was routed, yet frames still
                    // ran interpreted — so those frames belong to code the JIT
                    // was never asked about (module top level, generators).
                    // Reporting coverage off `functions_seen` alone would call
                    // this 100% and be wrong.
                    terminal::log(format!(
                        "      {}",
                        chalk(
                            "ninguna función ofrecida al JIT falló: estos frames \
                             corresponden a código que nunca se le ofrece"
                        )
                        .dim()
                    ));
                }
            }
        }

        // Noise last: it qualifies everything above it.
        if let Some(exec) = self.execute {
            let spread = exec.spread();
            if exec.cv() >= CV_UNRELIABLE || spread >= CV_UNRELIABLE {
                terminal::blank();
                terminal::log(format!(
                    "  {} {}",
                    chalk("⚠").yellow(),
                    chalk(format!(
                        "spread {} — A/B suelto no confiable, usar método pareado",
                        fmt_pct(spread)
                    ))
                    .yellow()
                ));
                if let Some(note) = self.cpu.as_ref().and_then(throttle_note) {
                    terminal::log(format!("     {}", chalk(note).dim()));
                }
            }
        }
    }
}
