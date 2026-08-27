pub mod resolver;
pub mod cache;
mod check;
mod compile;
mod core;
mod error;
mod execute;
pub mod fmt;
pub mod hash;
pub mod import_collector;
mod lockfile;
pub mod module_precompile;
mod opts;
mod parse;
mod quiet_parse;
pub mod stdlib_loader;
pub mod wrc;

mod lex;

pub use check::check as phase_check;
pub use compile::{CompileOutput, CACHE_FORMAT_VERSION};
pub use core::core_protos_owned;
pub use error::PipelineError;
pub use execute::{execute, execute_with_caps};
pub use lex::lex as phase_lex;
pub use opts::{parse_debug_opt, CapabilitySet, DebugFlags, RunOpts};
pub use parse::parse as phase_parse;

type PipelineResult<T> = Result<T, PipelineError>;

pub fn canonicalize_path(path: &str) -> PipelineResult<String> {
    std::path::Path::new(path)
        .canonicalize()
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|e| PipelineError::fatal(format!("cannot resolve '{}': {}", path, e)))
}

pub fn read_source_file(path: &str) -> PipelineResult<String> {
    read_source(path)
}

pub fn run(opts: &RunOpts) -> PipelineResult<()> {
    if wrc::is_wrc(&opts.file_path) {
        return run_wrc(opts);
    }

    let source = if let Some(ref s) = opts.eval {
        s.clone()
    } else {
        read_source(&opts.file_path)?
    };
    let compiled = if opts.eval.is_none() && !opts.debug.any() && !opts.strict {
        compile_source_cached(&source, &opts.file_path, opts.verbose)?
    } else {
        compile_source(
            &source,
            &opts.file_path,
            opts.verbose,
            &opts.debug,
            opts.strict,
        )?
    };
    if opts.eval.is_none() {
        lockfile::sync_lockfile(&opts.file_path, &compiled.graph_artifact)?;
    }
    if opts.no_run {
        return Ok(());
    }
    let mut debug = opts.debug.clone();
    if opts.trace {
        debug.trace = true;
    }

    execute_with_caps(
        compiled.entry_proto,
        compiled.precompiled,
        &source,
        &opts.file_path,
        &debug,
        opts.capabilities.clone(),
    )
}

fn run_wrc(opts: &RunOpts) -> PipelineResult<()> {
    let artifact = wrc::read_wrc(&opts.file_path)?;
    let compiled = cache::compile_output_from_graph(artifact)?;

    if opts.no_run {
        return Ok(());
    }
    let mut debug = opts.debug.clone();
    if opts.trace {
        debug.trace = true;
    }

    execute_with_caps(
        compiled.entry_proto,
        compiled.precompiled,
        "",
        &opts.file_path,
        &debug,
        opts.capabilities.clone(),
    )
}

pub fn compile_source_for_build(
    source: &str,
    path: &str,
    verbose: bool,
    debug: &DebugFlags,
) -> PipelineResult<CompileOutput> {
    compile_source(source, path, verbose, debug, false)
}

fn compile_source(
    source: &str,
    path: &str,
    verbose: bool,
    debug: &DebugFlags,
    strict: bool,
) -> PipelineResult<CompileOutput> {
    let canonical_path = std::path::Path::new(path)
        .canonicalize()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_owned());
    let path = canonical_path.as_str();
    let (tokens, lexeme_buf) = lex::lex(source, path, verbose, debug)?;
    let program = parse::parse(tokens, lexeme_buf, source, path, verbose, debug)?;
    let check_result = check::check(&program, source, debug, strict)?;
    let compiled = compile::compile(&program, source, check_result, verbose, debug)?;

    Ok(compiled)
}

fn compile_source_cached(source: &str, path: &str, verbose: bool) -> PipelineResult<CompileOutput> {
    let cache_path = cache::compile_cache_path(path);

    match cache::load_cached_graph(&cache_path, compile::CACHE_FORMAT_VERSION, source) {
        Ok(Some(graph_artifact)) => {
            if verbose {
                varn_term::terminal::tagged("Varn", "compile cache hit");
            }
            return cache::compile_output_from_graph(graph_artifact);
        }
        Ok(None) => {}
        Err(e) => {
            if verbose {
                varn_term::terminal::tagged(
                    "Varn",
                    format_args!("compile cache read skipped: {e}"),
                );
            }
        }
    }

    if verbose {
        varn_term::terminal::tagged("Varn", "compile cache miss");
    }

    let compiled = compile_source(source, path, verbose, &DebugFlags::default(), false)?;
    if let Err(e) = cache::store_cached_graph(&cache_path, &compiled.graph_artifact) {
        if verbose {
            varn_term::terminal::tagged("Varn", format_args!("compile cache write skipped: {e}"));
        }
    }
    Ok(compiled)
}

fn read_source(path: &str) -> PipelineResult<String> {
    std::fs::read_to_string(path)
        .map_err(|e| PipelineError::fatal(format!("error[io]: cannot read '{}': {}", path, e)))
}
