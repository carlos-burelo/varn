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
}

pub fn run_bench(path: &str, eval: Option<&str>, opts: &BenchOpts) -> Result<(), CliError> {
    if eval.is_none() && crate::pipeline::wrc::is_wrc(path) {
        return compiled::run(path, opts);
    }
    source::run(path, eval, opts)
}
