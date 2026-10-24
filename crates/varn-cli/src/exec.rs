use crate::debug_print;
use crate::cli::PermissionArgs;
use crate::error::CliError;
use varn_pipeline::{portable, CachePolicy, DebugFlags, Executable, Policy, Request, Session, Source};

pub fn cache_policy(debug: &DebugFlags) -> CachePolicy {
    if debug.any() {
        CachePolicy::Bypass
    } else {
        CachePolicy::Use
    }
}

pub fn compile(
    session: &Session,
    source: Source,
    cache: CachePolicy,
    debug: &DebugFlags,
    verbose: bool,
) -> Result<Executable, CliError> {
    let compilation = session.compile(source, Request::executable(cache));
    debug_print::report(&compilation, debug, verbose);
    compilation.into_executable().map_err(CliError::from)
}

pub fn load(
    session: &Session,
    file_path: &str,
    debug: &DebugFlags,
    verbose: bool,
) -> Result<Executable, CliError> {
    if portable::is_portable(file_path) {
        let graph = portable::read_portable(file_path)?;
        return Executable::from_graph(graph).map_err(CliError::from);
    }
    let source = Source::file(file_path)?;
    compile(session, source, cache_policy(debug), debug, verbose)
}

pub fn policy(permissions: &PermissionArgs, debug: &DebugFlags, silent: bool) -> Policy {
    Policy {
        capabilities: crate::policy::capabilities(permissions),
        trace: debug.trace,
        gc: debug.gc,
        silent,
    }
}

pub fn run(session: &Session, executable: Executable, policy: Policy) -> Result<(), CliError> {
    varn_pipeline::execute(session, executable, policy).map_err(CliError::from)
}
