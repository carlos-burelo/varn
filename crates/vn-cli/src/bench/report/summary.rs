use std::time::Duration;

use vn_jit::JitStatsSnapshot;

use super::fmt::{fmt_bytes, fmt_dur, percent, share, short_path, truncate_middle, DurScale};
use super::plain::{
    Align::{Left, Right},
    Report, Table,
};
use crate::bench::record::{BuildRecord, JitRecord, PhaseRecord, Record};
use crate::bench::stats::{PhaseStats, CV_UNRELIABLE};

pub struct BuildId {
    pub profile: &'static str,
    pub backend: &'static str,
    pub commit: Option<&'static str>,
}

impl BuildId {
    pub fn detect() -> Self {
        let backend = if vn_core::env::process().no_jit || !vn_jit::clif::enabled() {
            "interp"
        } else {
            "clif"
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
        self.backend == "interp"
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

pub struct SourceInfo {
    pub lines: usize,
    pub tokens: usize,
}

pub struct Summary<'a> {
    pub path: &'a str,
    pub runs: usize,
    pub bytes: u64,
    pub source: Option<SourceInfo>,
    pub phases: &'a [PhaseStats],
    pub e2e: Option<&'a PhaseStats>,
    pub precompile: Option<Duration>,
    pub split: Option<ExecSplit>,
    pub jit: &'a JitStatsSnapshot,
    pub top_blocker: Option<(String, String)>,
    pub tiered_during_window: u64,
    pub cpu: Option<crate::cpu_freq::CpuFreq>,
    pub silenced_output: bool,
}

impl Summary<'_> {
    pub fn record(&self) -> Record {
        let build = BuildId::detect();
        let jit = self.jit;
        let mut record = Record::new(
            short_path(self.path),
            BuildRecord {
                profile: build.profile.to_owned(),
                backend: build.backend.to_owned(),
                commit: build.commit.map(str::to_owned),
            },
            JitRecord {
                compiled: jit.compile_success,
                seen: jit.functions_seen(),
                blocked: jit.gate_rejected + jit.compile_fail,
                warmup_frames: jit.never_compiled_frames(),
                compiled_in_window: self.tiered_during_window,
                compile_ns: jit.total_compile_time_ns,
            },
        );
        record.phases = self.phases.iter().map(PhaseRecord::of).collect();
        record.e2e = self.e2e.map(PhaseRecord::of);
        record
    }

    pub fn fill(&self, report: &mut Report) {
        let build = BuildId::detect();

        let mut meta = Table::pairs();
        meta.row(["bench".to_owned(), short_path(self.path)]);
        meta.row(["runs".to_owned(), self.runs.to_string()]);
        meta.row([
            "build".to_owned(),
            format!(
                "{} {} {}",
                build.profile,
                build.backend,
                build.commit.unwrap_or("-")
            ),
        ]);
        meta.row(["bytes".to_owned(), fmt_bytes(self.bytes)]);
        if let Some(source) = &self.source {
            meta.row(["lines".to_owned(), source.lines.to_string()]);
            meta.row(["tokens".to_owned(), source.tokens.to_string()]);
        }
        report.table(&meta);

        self.phases_table(report);
        self.result_table(report);
        self.jit_table(report, &build);
    }

    fn phases_table(&self, report: &mut Report) {
        let total_p50: Duration = self.phases.iter().map(|p| p.p50).sum();
        let total_mean: Duration = self.phases.iter().map(|p| p.mean()).sum();
        let scale = DurScale::for_column(
            self.phases
                .iter()
                .chain(self.e2e)
                .flat_map(|p| [p.min, p.p50, p.mean(), p.p95, p.max])
                .chain([total_p50, total_mean]),
        );
        report.title("phases");
        let mut table = Table::new(
            [
                "phase",
                &scale.header("min"),
                &scale.header("p50"),
                &scale.header("mean"),
                &scale.header("p95"),
                &scale.header("max"),
                "cv%",
                "ci95%",
                "out",
                "share%",
            ],
            [
                Left, Right, Right, Right, Right, Right, Right, Right, Right, Right,
            ],
        );
        let nanos = |d: Duration| d.as_nanos() as u64;
        for phase in self.phases {
            table.row([
                phase.name.to_owned(),
                scale.num(phase.min),
                scale.num(phase.p50),
                scale.num(phase.mean()),
                scale.num(phase.p95),
                scale.num(phase.max),
                percent(phase.cv()),
                percent(phase.ci_ratio()),
                phase.outliers.to_string(),
                share(nanos(phase.p50), nanos(total_p50)),
            ]);
        }
        table.row([
            "total".to_owned(),
            "-".to_owned(),
            scale.num(total_p50),
            scale.num(total_mean),
            "-".to_owned(),
            "-".to_owned(),
            "-".to_owned(),
            "-".to_owned(),
            "-".to_owned(),
            "100.0".to_owned(),
        ]);
        if let Some(e2e) = self.e2e {
            table.row([
                e2e.name.to_owned(),
                scale.num(e2e.min),
                scale.num(e2e.p50),
                scale.num(e2e.mean()),
                scale.num(e2e.p95),
                scale.num(e2e.max),
                percent(e2e.cv()),
                percent(e2e.ci_ratio()),
                e2e.outliers.to_string(),
                "-".to_owned(),
            ]);
        }
        report.table(&table);
    }

    fn result_table(&self, report: &mut Report) {
        let reference = self
            .e2e
            .map(|e| e.p50)
            .unwrap_or_else(|| self.phases.iter().map(|p| p.p50).sum());
        let per_second = 1_000_000_000.0 / reference.as_nanos().max(1) as f64;
        report.title("result");
        let mut table = Table::pairs();
        table.row(["p50".to_owned(), fmt_dur(reference)]);
        table.row(["throughput".to_owned(), format!("{per_second:.1}/s")]);
        if let Some(peak) = crate::peak_memory::bytes() {
            table.row(["peak memory".to_owned(), fmt_bytes(peak)]);
        }
        if let Some(precompile) = self.precompile {
            table.row(["precompile".to_owned(), fmt_dur(precompile)]);
        }
        if self.silenced_output {
            table.row([
                "stdout".to_owned(),
                "silenced, pass --show-output to see it".to_owned(),
            ]);
        }
        report.table(&table);
    }

    fn jit_table(&self, report: &mut Report, build: &BuildId) {
        report.title("jit");
        if build.jit_disabled() {
            report.line("disabled");
            return;
        }
        let jit = self.jit;
        let mut table = Table::pairs();
        table.row([
            "compiled".to_owned(),
            format!("{}/{}", jit.compile_success, jit.functions_seen()),
        ]);
        table.row([
            "blocked".to_owned(),
            (jit.gate_rejected + jit.compile_fail).to_string(),
        ]);
        table.row([
            "top blocker".to_owned(),
            self.top_blocker
                .as_ref()
                .map(|(name, reason)| truncate_middle(&format!("{name}: {reason}"), 72))
                .unwrap_or_else(|| "-".to_owned()),
        ]);
        table.row([
            "warmup frames".to_owned(),
            jit.never_compiled_frames().to_string(),
        ]);
        table.row([
            "compiled in window".to_owned(),
            self.tiered_during_window.to_string(),
        ]);
        if let Some(split) = &self.split {
            table.row(["compile in run".to_owned(), fmt_dur(split.compile)]);
        }
        report.table(&table);
    }

    pub fn warn(&self) {
        let noisy = self
            .phases
            .iter()
            .find(|p| p.name == "execute (warm)")
            .is_some_and(|p| p.cv() >= CV_UNRELIABLE);
        if noisy {
            eprintln!("warning: execute cv is above 10%, the run is not reliable");
        }
        if let Some(cpu) = &self.cpu
            && cpu.max_mhz > 0
            && (cpu.cur_mhz as f64) / (cpu.max_mhz as f64) < 0.90
        {
            eprintln!(
                "warning: cpu throttled at {} of {} MHz",
                cpu.cur_mhz, cpu.max_mhz
            );
        }
    }
}
