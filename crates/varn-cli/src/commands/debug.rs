use crate::{cli::DebugArgs, error::CliError, pipeline};

pub fn execute(session: &pipeline::Session, args: DebugArgs) -> Result<(), CliError> {
    if args.list_phases {
        varn_debug::print_phases();
        return Ok(());
    }

    if args.natives {
        crate::debug_print::natives();
        return Ok(());
    }

    let mut debug = parse_debug_opt(&args.phase)?;
    debug.fn_filter = args.fn_filter;

    let (file_path, eval) = match (args.file, args.eval) {
        (_, Some(code)) => (crate::source_name::EVAL.to_owned(), Some(code)),
        (Some(file), None) => (file, None),
        (None, None) => {
            return Err(CliError::usage(
                "Provide a file or inline code with --eval (--list-phases to see phases)",
            ))
        }
    };

    varn_inspect::run_for(&file_path, eval.as_deref(), &debug);

    let executable = match eval {
        Some(code) => crate::exec::compile(
            session,
            pipeline::Source::virtual_source(&file_path, &code),
            pipeline::CachePolicy::Bypass,
            &debug,
            false,
        )?,
        None => crate::exec::load(session, &file_path, &debug, false)?,
    };
    if debug.needs_execution() {
        crate::exec::run(
            session,
            executable,
            crate::exec::policy(&args.permissions, &debug, false),
        )?;
    }
    Ok(())
}

fn parse_debug_opt(spec: &str) -> Result<pipeline::DebugFlags, CliError> {
    varn_debug::flags::parse_debug_flags(spec).map_err(|e| CliError::usage(e.message))
}
