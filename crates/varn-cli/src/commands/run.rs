use crate::{cli::RunArgs, error::CliError, opts::RunOpts, pipeline};

pub fn execute(args: RunArgs) -> Result<(), CliError> {
    let debug = crate::opts::parse_debug_opt(args.debug.as_deref())?;
    pipeline::run(&RunOpts {
        file_path: args.file,
        eval: None,
        verbose: args.verbose,
        no_run: args.no_run,
        debug,
        trace: args.trace,
        strict: args.strict,
    })
}
