//! `vn bench` — phase timing, Cranelift coverage, runtime profile.

pub mod compiled;
pub mod harness;
pub mod report;
pub mod source;
pub mod stats;

use crate::error::CliError;

/// Knobs that change what the report shows, not what it measures.
pub struct BenchOpts {
    pub runs: usize,
    pub show_output: bool,
    pub verbose: bool,
    pub all_rows: bool,
    /// `--min-clif-coverage`: floor, in percent of observed frames.
    pub min_clif_coverage: Option<f64>,
}

pub fn run_bench(path: &str, eval: Option<&str>, opts: &BenchOpts) -> Result<(), CliError> {
    if eval.is_none() && crate::pipeline::wrc::is_wrc(path) {
        return compiled::run(path, opts);
    }
    source::run(path, eval, opts)
}

/// Turn `--min-clif-coverage` into an exit code.
///
/// Shared by both bench entry points so the threshold means one thing. The
/// numerator is `machine_code_frames` — frames that executed machine code,
/// OSR rescues included — which is the same figure the headline prints; a
/// guard that measured something else than the report would be worse than no
/// guard.
pub fn enforce_coverage_floor(
    jit: &varn_vm::varn_jit::JitStatsSnapshot,
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
