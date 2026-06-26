use crate::{cli::EvalArgs, error::CliError, pipeline};
use varn_pipeline::RunOpts;

pub fn execute(args: EvalArgs) -> Result<(), CliError> {
    pipeline::run(&RunOpts {
        file_path: "(eval)".to_owned(),
        eval: Some(args.code),
        verbose: args.verbose,
        no_run: false,
        debug: Default::default(),
        trace: false,
        strict: false,
    })
}
