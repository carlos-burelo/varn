use crate::{cli::EvalArgs, error::CliError, pipeline};

pub fn execute(session: &pipeline::Session, args: EvalArgs) -> Result<(), CliError> {
    let debug = pipeline::DebugFlags::default();
    let executable = crate::exec::compile(
        session,
        pipeline::Source::virtual_source(crate::source_name::EVAL, &args.code),
        pipeline::CachePolicy::Bypass,
        &debug,
        args.verbose,
    )?;
    crate::exec::run(
        session,
        executable,
        &debug,
        pipeline::CapabilitySet::allow_all(),
        false,
    )
}
