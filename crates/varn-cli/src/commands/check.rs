use crate::{cli::CheckArgs, error::CliError, opts::RunOpts, pipeline};

pub fn execute(args: CheckArgs) -> Result<(), CliError> {
    let debug = crate::opts::parse_debug_opt(args.debug.as_deref())?;
    pipeline::run(&RunOpts {
        file_path: args.file,
        eval: None,
        verbose: args.verbose,
        no_run: true,
        debug,
        trace: false,
        strict: args.strict,
    })
}
