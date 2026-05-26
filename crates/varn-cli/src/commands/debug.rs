use crate::{cli::DebugArgs, error::CliError, opts::RunOpts, pipeline};

pub fn execute(args: DebugArgs) -> Result<(), CliError> {
    let debug = crate::opts::parse_debug_opt(Some(&args.phase))?;

    let (file_path, eval) = match (args.file, args.eval) {
        (_, Some(code)) => ("(eval)".to_owned(), Some(code)),
        (Some(file), None) => (file, None),
        (None, None) => {
            return Err(CliError::usage(
                "Provide a file or inline code with --eval",
            ))
        }
    };

    pipeline::run(&RunOpts {
        file_path,
        eval,
        verbose: false,
        no_run: true,
        debug,
        trace: false,
        strict: false,
    })
}
