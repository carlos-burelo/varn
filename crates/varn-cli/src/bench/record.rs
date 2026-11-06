use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::stats::PhaseStats;
use crate::error::CliError;

const SCHEMA: u32 = 1;

#[derive(Serialize, Deserialize)]
pub struct PhaseRecord {
    pub name: String,
    pub runs: usize,
    pub min_ns: u64,
    pub p50_ns: u64,
    pub mean_ns: u64,
    pub p95_ns: u64,
    pub max_ns: u64,
    pub stddev_ns: u64,
    pub ci95_ns: u64,
    pub outliers: usize,
    pub samples_ns: Vec<u64>,
}

impl PhaseRecord {
    pub fn of(stats: &PhaseStats) -> Self {
        Self {
            name: stats.name.to_owned(),
            runs: stats.runs,
            min_ns: nanos(stats.min),
            p50_ns: nanos(stats.p50),
            mean_ns: nanos(stats.mean()),
            p95_ns: nanos(stats.p95),
            max_ns: nanos(stats.max),
            stddev_ns: nanos(stats.stddev),
            ci95_ns: nanos(stats.ci95),
            outliers: stats.outliers,
            samples_ns: stats.samples.iter().map(|s| nanos(*s)).collect(),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct BuildRecord {
    pub profile: String,
    pub backend: String,
    pub commit: Option<String>,
}

#[derive(Serialize, Deserialize, Default)]
pub struct JitRecord {
    pub compiled: u64,
    pub seen: u64,
    pub blocked: u64,
    pub warmup_frames: u64,
    pub compiled_in_window: u64,
    pub compile_ns: u64,
}

#[derive(Serialize, Deserialize)]
pub struct FunctionRecord {
    pub function: String,
    pub file: String,
    pub self_samples: u64,
    pub total_samples: u64,
}

#[derive(Serialize, Deserialize)]
pub struct ProfileRecord {
    pub hz: u32,
    pub samples: u64,
    pub compiled_samples: u64,
    pub functions: Vec<FunctionRecord>,
}

#[derive(Serialize, Deserialize)]
pub struct Record {
    pub schema: u32,
    pub bench: String,
    pub build: BuildRecord,
    pub phases: Vec<PhaseRecord>,
    pub e2e: Option<PhaseRecord>,
    pub jit: JitRecord,
    pub profile: Option<ProfileRecord>,
    #[serde(default)]
    pub comparison: Vec<Change>,
}

impl Record {
    pub fn new(bench: String, build: BuildRecord, jit: JitRecord) -> Self {
        Self {
            schema: SCHEMA,
            bench,
            build,
            phases: Vec::new(),
            e2e: None,
            jit,
            profile: None,
            comparison: Vec::new(),
        }
    }

    pub fn load(path: &str) -> Result<Self, CliError> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| CliError::fatal(format!("could not read baseline {path}: {e}")))?;
        let record: Record = serde_json::from_str(&text)
            .map_err(|e| CliError::fatal(format!("baseline {path} is not a bench record: {e}")))?;
        if record.schema != SCHEMA {
            return Err(CliError::fatal(format!(
                "baseline {path} has schema {}, this build writes schema {SCHEMA}",
                record.schema
            )));
        }
        Ok(record)
    }

    pub fn json(&self) -> String {
        serde_json::to_string_pretty(self).expect("a bench record always serializes")
    }

    fn phase(&self, name: &str) -> Option<&PhaseRecord> {
        self.phases
            .iter()
            .chain(self.e2e.iter())
            .find(|p| p.name == name)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Verdict {
    Regression,
    Improvement,
    Same,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Change {
    pub phase: String,
    pub baseline_p50_ns: u64,
    pub current_p50_ns: u64,
    pub delta_percent: f64,
    pub verdict: Verdict,
}

pub fn compare(baseline: &Record, current: &Record) -> Vec<Change> {
    let mut changes = Vec::new();
    for now in current.phases.iter().chain(current.e2e.iter()) {
        let Some(before) = baseline.phase(&now.name) else {
            continue;
        };
        let delta = now.mean_ns as f64 - before.mean_ns as f64;
        let noise = (now.ci95_ns + before.ci95_ns) as f64;
        let verdict = if delta.abs() <= noise {
            Verdict::Same
        } else if delta > 0.0 {
            Verdict::Regression
        } else {
            Verdict::Improvement
        };
        changes.push(Change {
            phase: now.name.clone(),
            baseline_p50_ns: before.p50_ns,
            current_p50_ns: now.p50_ns,
            delta_percent: if before.p50_ns == 0 {
                0.0
            } else {
                (now.p50_ns as f64 - before.p50_ns as f64) / before.p50_ns as f64 * 100.0
            },
            verdict,
        });
    }
    changes
}

fn nanos(d: Duration) -> u64 {
    d.as_nanos() as u64
}
