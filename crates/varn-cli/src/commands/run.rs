use crate::{cli::RunArgs, error::CliError, pipeline};

pub fn execute(session: &pipeline::Session, args: RunArgs) -> Result<(), CliError> {
    let (file_path, eval) = match (args.file.as_ref(), args.eval.as_ref()) {
        (_, Some(code)) => (crate::source_name::EVAL.to_owned(), Some(code.clone())),
        (Some(file), None) => (file.clone(), None),
        (None, None) => return Err(CliError::usage("Provide a file or inline code with --eval")),
    };

    if args.compare_tiers {
        if eval.is_some() {
            return Err(CliError::usage(
                "--compare-tiers needs a file: each tier runs as its own process",
            ));
        }
        return crate::commands::compare_tiers::execute(&file_path, &args.script_args);
    }

    let mut debug = varn_pipeline::DebugFlags::default();
    debug.trace = args.trace;

    let executable = match eval {
        Some(code) => crate::exec::compile(
            session,
            varn_pipeline::Source::virtual_source(&file_path, &code),
            varn_pipeline::CachePolicy::Bypass,
            &debug,
            args.verbose,
        )?,
        None => crate::exec::load(session, &file_path, &debug, args.verbose)?,
    };
    crate::exec::run(
        session,
        executable,
        crate::exec::policy(&args.permissions, &debug, false),
    )
}
