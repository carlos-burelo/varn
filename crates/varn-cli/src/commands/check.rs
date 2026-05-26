use crate::{cli::CheckArgs, error::CliError, opts::RunOpts, pipeline};

pub fn execute(args: CheckArgs) -> Result<(), CliError> {
    pipeline::run(&RunOpts {
        file_path: args.file,
        eval: None,
        verbose: args.verbose,
        no_run: true,
        debug: Default::default(),
        trace: false,
        strict: args.strict,
    })
}
