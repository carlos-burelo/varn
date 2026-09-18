use crate::PipelineError;
use std::rc::Rc;
use varn_debug::flags::DebugFlags;

type PipelineResult<T> = Result<T, PipelineError>;

pub fn parse(
    tokens: Vec<varn_core::Token>,
    lexeme_buf: Rc<[u8]>,
    source: &str,
    path: &str,
    verbose: bool,
    debug: &DebugFlags,
) -> PipelineResult<(varn_core::ast::Program, varn_core::AtomInterner)> {
    // The entry file used to parse through its own throwaway `AtomInterner`,
    // a genuinely separate path from `module_resolver`/`with_resolver` below
    // it (imports go through `DiskResolver::parse_and_cache`, this file did
    // not). Two tables meant the entry file's `Atom`s and an imported
    // module's `Atom`s were never comparable — seed this parse from the
    // resolver's shared table instead, and publish the grown result back, so
    // the root file and everything it imports share one `Atom` space.
    let interner = crate::resolver::with_resolver(|r| r.interner_snapshot());
    // TODO(fase1-componente2): varn-pipeline's own `parse` still returns
    // `(Program, AtomInterner)`; threading `AstArena` through its public
    // signature (and every caller of *this* function) is later-task scope.
    let (program, interner, _arena) = varn_parser::parse(tokens, lexeme_buf, path, interner)
        .map_err(|errs| {
        let msgs: Vec<String> = errs
            .iter()
            .map(|e| varn_core::diagnostics::format_diagnostic(e, source))
            .collect();
        let error_count = errs.len();
        let footer = format!(
            "\n{}: could not compile `{}` due to {} previous error{}",
            varn_core::term::chalk::chalk("error").red().bold(),
            path,
            error_count,
            if error_count > 1 { "s" } else { "" }
        );
        PipelineError::new(3, format!("{}\n{}", msgs.join("\n"), footer))
    })?;
    // Publish the entry file's own atoms into the shared table before any
    // import gets resolved: an import that reaches back into this file's
    // exports (a re-export cycle) must see these atoms, not a stale
    // pre-entry-file snapshot.
    crate::resolver::with_resolver(|r| r.set_interner(interner.clone()));

    if verbose {
        varn_core::term::terminal::tagged(
            "Varn",
            format_args!("parsed {} top-level statements", program.body.len()),
        );
    }

    if debug.ast {
        varn_debug::ast::debug_ast(&program, &interner);
    }

    if debug.modules {
        varn_debug::modules::debug_modules(&program);
    }

    Ok((program, interner))
}
