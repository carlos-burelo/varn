use crate::{cli::DebugArgs, error::CliError, pipeline};
use varn_pipeline::RunOpts;

pub fn execute(args: DebugArgs) -> Result<(), CliError> {
    if args.list_phases {
        varn_debug::print_phases();
        return Ok(());
    }

    let mut debug = varn_pipeline::parse_debug_opt(Some(&args.phase))?;
    debug.fn_filter = args.fn_filter;

    let (file_path, eval) = match (args.file, args.eval) {
        (_, Some(code)) => ("(eval)".to_owned(), Some(code)),
        (Some(file), None) => (file, None),
        (None, None) => {
            return Err(CliError::usage(
                "Provide a file or inline code with --eval (--list-phases to see phases)",
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
