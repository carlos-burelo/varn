use crate::{cli::RunArgs, error::CliError, pipeline};
use varn_pipeline::RunOpts;

pub fn execute(args: RunArgs) -> Result<(), CliError> {
    pipeline::run(&RunOpts {
        file_path: args.file,
        eval: None,
        verbose: args.verbose,
        no_run: false,
        debug: Default::default(),
        trace: args.trace,
        strict: args.strict,
    })
}
