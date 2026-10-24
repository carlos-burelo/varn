pub mod cache;
mod check;
mod compile;
mod core;
mod debug_sink;
mod error;
mod execute;
pub mod fmt;
pub mod import_collector;
pub mod module_precompile;
mod opts;
mod session;
mod source;
mod parse;
pub mod portable;
mod quiet_parse;
pub use varn_resolver::ModuleResolver;
pub mod stdlib_loader;

mod class_link;
mod layer_link;
mod lex;

pub use check::check as phase_check;
pub use check::collect_test_targets;
pub use class_link::verify_foreign_layouts;
pub use compile::CompileOutput;
pub use compile::{
    compile_tir, emit_tir, inject_star_reexports, report_export_collisions, resolve_import_slots,
    sorted_export_names, tir_import_sources,
};
pub use core::core_protos_owned;
pub use debug_sink::{DebugSink, NullSink};
pub use error::{DiagnosticReport, Failure, RuntimeFailure};
pub use execute::{boot_machine, enter_main, execute, execute_with_caps};
pub use layer_link::verify_synthetic_imports;
pub use lex::lex as phase_lex;
pub use opts::{CapabilitySet, DebugFlags, RunOpts};
pub use session::Session;
pub use source::Source;
pub use varn_project::Anchor;
pub use parse::parse as phase_parse;

type PipelineResult<T> = Result<T, Failure>;

pub fn run(
    session: &Session,
    opts: &RunOpts,
    sink: &dyn debug_sink::DebugSink,
) -> PipelineResult<()> {
    let resolver = session.resolver();
    if portable::is_portable(&opts.file_path) {
        return run_portable(session, opts);
    }

    let loaded = match &opts.eval {
        Some(code) => Source::virtual_source(&opts.file_path, code),
        None => Source::file(&opts.file_path)?,
    };
    let path = loaded.path();
    let source = match &opts.append {
        Some(extra) => format!("{}\n{extra}\n", loaded.text()),
        None => loaded.text().to_owned(),
    };
    let compiled = if opts.eval.is_none() && opts.append.is_none() && !opts.debug.any() {
        compile_source_cached(&source, path, opts.verbose, resolver, sink)?
    } else {
        compile_source(
            &source,
            path,
            opts.verbose,
            &opts.debug,
            resolver,
            sink,
        )?
    };
    if opts.no_run {
        return Ok(());
    }
    let mut debug = opts.debug.clone();
    if opts.trace {
        debug.trace = true;
    }

    execute_with_caps(
        session,
        compiled.entry_proto,
        compiled.precompiled,
        &source,
        &opts.file_path,
        &debug,
        opts.capabilities.clone(),
        opts.print_silent,
    )
}

fn run_portable(session: &Session, opts: &RunOpts) -> PipelineResult<()> {
    let artifact = portable::read_portable(&opts.file_path)?;
    let compiled = cache::compile_output_from_graph(artifact)?;

    if opts.no_run {
        return Ok(());
    }
    let mut debug = opts.debug.clone();
    if opts.trace {
        debug.trace = true;
    }

    execute_with_caps(
        session,
        compiled.entry_proto,
        compiled.precompiled,
        "",
        &opts.file_path,
        &debug,
        opts.capabilities.clone(),
        opts.print_silent,
    )
}

pub fn compile_source_for_build(
    source: &str,
    path: &str,
    verbose: bool,
    debug: &DebugFlags,
    session: &ModuleResolver,
    sink: &dyn debug_sink::DebugSink,
) -> PipelineResult<CompileOutput> {
    compile_source(source, path, verbose, debug, session, sink)
}

fn compile_source(
    source: &str,
    path: &str,
    verbose: bool,
    debug: &DebugFlags,
    session: &ModuleResolver,
    sink: &dyn debug_sink::DebugSink,
) -> PipelineResult<CompileOutput> {
    let (tokens, lexeme_buf) = lex::lex(source, path, verbose, debug, sink)?;
    let (program, arena, interner) =
        parse::parse(tokens, lexeme_buf, source, path, verbose, debug, sink)?;
    let check_result = check::check(&program, &arena, interner, source, debug, session, sink)?;
    let compiled = compile::compile(
        &program,
        &arena,
        source,
        check_result,
        verbose,
        debug,
        session,
        sink,
    )?;

    Ok(compiled)
}

fn compile_source_cached(
    source: &str,
    path: &str,
    verbose: bool,
    session: &ModuleResolver,
    sink: &dyn debug_sink::DebugSink,
) -> PipelineResult<CompileOutput> {
    let cache_path = cache::compile_cache_path(path);

    match cache::load_cached_graph(&cache_path, source, verbose) {
        Ok(Some(graph_artifact)) => {
            if verbose {
                varn_core::term::terminal::tagged("Varn", "compile cache hit");
            }
            return cache::compile_output_from_graph(graph_artifact);
        }
        Ok(None) => {}
        Err(e) => {
            if verbose {
                varn_core::term::terminal::tagged(
                    "Varn",
                    format_args!("compile cache read skipped: {e}"),
                );
            }
        }
    }

    if verbose {
        varn_core::term::terminal::tagged("Varn", "compile cache miss");
    }

    let compiled = compile_source(source, path, verbose, &DebugFlags::default(), session, sink)?;
    if let Err(e) = cache::store_cached_graph(&cache_path, &compiled.graph_artifact)
        && verbose
    {
        varn_core::term::terminal::tagged("Varn", format_args!("compile cache write skipped: {e}"));
    }
    Ok(compiled)
}
