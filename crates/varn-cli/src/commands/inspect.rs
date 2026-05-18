use crate::{cli::InspectArgs, error::CliError, opts::RunOpts, pipeline};

pub fn execute(args: InspectArgs) -> Result<(), CliError> {
    let debug = crate::opts::parse_debug_opt(Some(&args.phases))?;

    let (file_path, eval) = match (args.file, args.eval) {
        (_, Some(code)) => ("(eval)".to_owned(), Some(code)),
        (Some(file), None) => (file, None),
        (None, None) => {
            return Err(CliError::usage(
                "Debes proporcionar un archivo a inspeccionar o código directo con --eval",
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
    })
}
