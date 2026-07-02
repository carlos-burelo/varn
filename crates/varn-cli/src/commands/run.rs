use crate::{cli::RunArgs, error::CliError, pipeline};
use varn_pipeline::RunOpts;

pub fn execute(args: RunArgs) -> Result<(), CliError> {
    let (file_path, eval) = match (args.file, args.eval) {
        (_, Some(code)) => ("(eval)".to_owned(), Some(code)),
        (Some(file), None) => (file, None),
        (None, None) => return Err(CliError::usage("Provide a file or inline code with --eval")),
    };

    pipeline::run(&RunOpts {
        file_path,
        eval,
        verbose: args.verbose,
        no_run: false,
        debug: Default::default(),
        trace: args.trace,
        strict: args.strict,
    })
}
