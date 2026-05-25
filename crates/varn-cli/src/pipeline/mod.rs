pub mod cache;
mod check;
mod compile;
mod core;
mod execute;
pub mod hash;
mod lex;
mod lockfile;
mod parse;
pub mod wrc;

use crate::error::CliError;
use crate::opts::RunOpts;
use varn_compiler::FunctionProto;
use varn_debug::flags::DebugFlags;

type PipelineResult<T> = Result<T, CliError>;

pub use compile::CompileOutput;
pub use core::core_protos_owned;
pub use execute::execute;

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

    execute(
        compiled.entry_proto,
        compiled.precompiled,
        &source,
        &opts.file_path,
        &debug,
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

    execute(
        compiled.entry_proto,
        compiled.precompiled,
        "",
        &opts.file_path,
        &debug,
    )
}

pub fn compile_file(
    path: &str,
    verbose: bool,
    debug: &DebugFlags,
) -> PipelineResult<FunctionProto> {
    let source = read_source(path)?;
    let compiled = compile_source(&source, path, verbose, debug, false)?;
    Ok(compiled.entry_proto)
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
    let (tokens, lexeme_buf) = lex::lex(source, path, verbose, debug);
    let mut program = parse::parse(tokens, lexeme_buf, source, path, verbose, debug)?;
    varn_core::assign_ast_ids(&mut program);
    let check_result = check::check(&program, source, debug, strict)?;
    let compiled = compile::compile(&program, source, check_result, verbose, debug)?;

    Ok(compiled)
}

fn compile_source_cached(source: &str, path: &str, verbose: bool) -> PipelineResult<CompileOutput> {
    let cache_path = cache::compile_cache_path(path);

    match cache::load_cached_graph(&cache_path, compile::CACHE_FORMAT_VERSION, source) {
        Ok(Some(graph_artifact)) => {
            if verbose {
                eprintln!("[Varn] compile cache hit");
            }
            return cache::compile_output_from_graph(graph_artifact);
        }
        Ok(None) => {}
        Err(e) => {
            if verbose {
                eprintln!("[Varn] compile cache read skipped: {e}");
            }
        }
    }

    if verbose {
        eprintln!("[Varn] compile cache miss");
    }

    let compiled = compile_source(source, path, verbose, &DebugFlags::default(), false)?;
    if let Err(e) = cache::store_cached_graph(&cache_path, &compiled.graph_artifact) {
        if verbose {
            eprintln!("[Varn] compile cache write skipped: {e}");
        }
    }
    Ok(compiled)
}

fn read_source(path: &str) -> PipelineResult<String> {
    std::fs::read_to_string(path)
        .map_err(|e| CliError::fatal(format!("error[io]: cannot read '{}': {}", path, e)))
}

pub fn lex_raw(source: &str, path: &str) -> (Vec<varn_core::Token>, std::rc::Rc<[u8]>) {
    lex::lex(source, path, false, &DebugFlags::default())
}

pub fn parse_raw(
    tokens: Vec<varn_core::Token>,
    lexeme_buf: std::rc::Rc<[u8]>,
    source: &str,
    path: &str,
) -> PipelineResult<varn_core::ast::Program> {
    parse::parse(
        tokens,
        lexeme_buf,
        source,
        path,
        false,
        &DebugFlags::default(),
    )
}
