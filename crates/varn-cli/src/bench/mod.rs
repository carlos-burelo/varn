pub mod compiled;
pub mod detail;
pub mod harness;
pub mod output;
pub mod record;
pub mod report;
pub mod source;
pub mod stats;

use crate::error::CliError;

pub struct ProfileOpts {
    pub hz: u32,
    pub out: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Text,
    Json,
}

pub struct BenchOpts {
    pub runs: usize,
    pub warmup: usize,
    pub show_output: bool,
    pub verbose: bool,
    pub format: Format,
    pub baseline: Option<String>,
    pub max_regression: Option<f64>,
    pub profile: Option<ProfileOpts>,
    pub min_clif_coverage: Option<f64>,
    pub capabilities: varn_pipeline::CapabilitySet,
}

pub fn run_bench(
    session: &varn_pipeline::Session,
    path: &str,
    eval: Option<&str>,
    opts: &BenchOpts,
) -> Result<(), CliError> {
    if eval.is_none() && crate::pipeline::portable::is_portable(path) {
        return compiled::run(session, path, opts);
    }
    source::run(session, path, eval, opts)
}

pub fn enforce_coverage_floor(
    jit: &varn_jit::JitStatsSnapshot,
    min_pct: Option<f64>,
) -> Result<(), CliError> {
    let Some(min_pct) = min_pct else {
        return Ok(());
    };
    let total = jit.total_frames();
    if total == 0 {
        return Err(CliError::fatal(
            "--min-clif-coverage: no frames were observed, so coverage is not defined",
        ));
    }
    let actual = jit.machine_code_ratio() * 100.0;
    if actual + f64::EPSILON < min_pct {
        return Err(CliError::fatal(format!(
            "clif coverage {actual:.2}% is below the required {min_pct:.2}% \
             ({} of {} frames ran as machine code)",
            jit.machine_code_frames(),
            total
        )));
    }
    Ok(())
}
