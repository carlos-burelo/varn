use crate::{cli::EvalArgs, error::CliError, opts::RunOpts, pipeline};

pub fn execute(args: EvalArgs) -> Result<(), CliError> {
    let debug = crate::opts::parse_debug_opt(args.debug.as_deref())?;
    pipeline::run(&RunOpts {
        file_path: "(eval)".to_owned(),
        eval: Some(args.code),
        verbose: args.verbose,
        no_run: false,
        debug,
        trace: false,
        strict: false,
    })
}
