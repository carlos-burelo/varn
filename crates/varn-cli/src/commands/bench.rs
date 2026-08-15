use crate::bench::{run_bench, BenchOpts};
use crate::{cli::BenchArgs, error::CliError};

pub fn execute(args: BenchArgs) -> Result<(), CliError> {
    if args.runs == 0 {
        return Err(CliError::usage("--runs must be at least 1"));
    }
    let (file_path, eval) = match (args.file, args.eval) {
        (_, Some(code)) => ("(eval)".to_owned(), Some(code)),
        (Some(file), None) => (file, None),
        (None, None) => return Err(CliError::usage("Provide a file or inline code with --eval")),
    };
    run_bench(
        &file_path,
        eval.as_deref(),
        &BenchOpts {
            runs: args.runs,
            show_output: args.show_output,
            verbose: args.verbose,
            all_rows: args.all_rows,
            min_clif_coverage: args.min_clif_coverage,
        },
    )
}
