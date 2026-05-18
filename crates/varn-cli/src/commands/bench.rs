use crate::{cli::BenchArgs, error::CliError};

pub fn execute(args: BenchArgs) -> Result<(), CliError> {
    if args.runs == 0 {
        return Err(CliError::usage("--runs debe ser al menos 1"));
    }
    let debug = crate::opts::parse_debug_opt(args.debug.as_deref())?;
    crate::bench_impl::run_bench(&args.file, args.runs, &debug, args.no_run, args.with_output)
}
