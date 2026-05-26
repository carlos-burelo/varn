use crate::{cli::BenchArgs, error::CliError};

pub fn execute(args: BenchArgs) -> Result<(), CliError> {
    if args.runs == 0 {
        return Err(CliError::usage("--runs must be at least 1"));
    }
    crate::bench_impl::run_bench(&args.file, args.runs, args.show_output)
}
