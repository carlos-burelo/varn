use crate::bench::{run_bench, BenchOpts, Format, ProfileOpts};
use crate::{
    cli::{BenchArgs, BenchFormat},
    error::CliError,
};

pub fn execute(session: &varn_pipeline::Session, args: BenchArgs) -> Result<(), CliError> {
    if args.runs == 0 {
        return Err(CliError::usage("--runs must be at least 1"));
    }
    if args.max_regression.is_some() && args.baseline.is_none() {
        return Err(CliError::usage("--max-regression needs --baseline"));
    }
    if args.profile_hz == 0 {
        return Err(CliError::usage("--profile-hz must be at least 1"));
    }
    let (file_path, eval) = match (args.file, args.eval) {
        (_, Some(code)) => (crate::source_name::EVAL.to_owned(), Some(code)),
        (Some(file), None) => (file, None),
        (None, None) => return Err(CliError::usage("Provide a file or inline code with --eval")),
    };
    run_bench(
        session,
        &file_path,
        eval.as_deref(),
        &BenchOpts {
            runs: args.runs,
            warmup: args.warmup,
            show_output: args.show_output,
            verbose: args.verbose,
            format: match args.format {
                BenchFormat::Text => Format::Text,
                BenchFormat::Json => Format::Json,
            },
            baseline: args.baseline,
            max_regression: args.max_regression,
            profile: (args.profile || args.profile_out.is_some()).then_some(ProfileOpts {
                hz: args.profile_hz,
                out: args.profile_out,
            }),
            min_clif_coverage: args.min_clif_coverage,
            capabilities: crate::policy::capabilities(&args.permissions),
        },
    )
}
