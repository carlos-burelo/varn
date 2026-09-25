use crate::PipelineError;
use std::sync::Arc;
use varn_debug::flags::DebugFlags;

type PipelineResult<T> = Result<T, PipelineError>;

/// Run `parse` in the resolver's atom space.
///
/// Every program the resolver will check must be parsed here. The parse is
/// seeded with the resolver's interner and, when it succeeds, the atoms it
/// added are published back before any import is resolved (an import that
/// reaches back into this file — a re-export cycle — must see them). A parse
/// through a private `AtomInterner::new()` gives this program atoms the
/// shared table assigns to other names: `n: int` in the file is then a `Set`
/// to the checker. A failed parse publishes nothing, so atoms other modules
/// coined meanwhile are kept.
pub fn in_shared_atoms<T, E>(
    parse: impl FnOnce(varn_core::AtomInterner) -> Result<(T, varn_core::AtomInterner), E>,
) -> Result<(T, varn_core::AtomInterner), E> {
    let interner = crate::resolver::with_resolver(|r| r.interner_snapshot());
    let (parsed, interner) = parse(interner)?;
    crate::resolver::with_resolver(|r| r.set_interner(interner.clone()));
    Ok((parsed, interner))
}

pub fn parse(
    tokens: Vec<varn_core::Token>,
    lexeme_buf: Arc<[u8]>,
    source: &str,
    path: &str,
    verbose: bool,
    debug: &DebugFlags,
) -> PipelineResult<(
    varn_core::ast::Program,
    varn_core::ast::AstArena,
    varn_core::AtomInterner,
)> {
    let (program, arena, interner) = in_shared_atoms(|interner| {
        varn_parser::parse(tokens, lexeme_buf, path, interner)
            .map(|(program, interner, arena)| ((program, arena), interner))
    })
    .map(|((program, arena), interner)| (program, arena, interner))
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

    if verbose {
        varn_core::term::terminal::tagged(
            "Varn",
            format_args!("parsed {} top-level statements", program.body.len()),
        );
    }

    if debug.ast {
        varn_debug::ast::debug_ast(&program, &arena, &interner);
    }

    if debug.modules {
        varn_debug::modules::debug_modules(&program, &arena);
    }

    Ok((program, arena, interner))
}
