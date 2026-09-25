//! Lex + parse for the internal compiles that report through `Result`.
//!
//! The `lex`/`parse` phases render diagnostics to the terminal, honour
//! `--verbose` and feed the `-p tokens` dump -- correct for a user-invoked run,
//! wrong for stdlib bundling and module precompilation, which must stay silent
//! and hand a message back to their caller. This is the driver for those.

use varn_core::ast::{AstArena, Program};

pub(crate) fn parse_module(
    source: &str,
    path: &str,
    label: &str,
) -> Result<(Program, AstArena, varn_core::AtomInterner), String> {
    let (tokens, lexeme_buf, _lex_errs) = varn_lexer::scan(source, path);
    let ((program, arena), interner) = crate::parse::in_shared_atoms(|interner| {
        varn_parser::parse(tokens, lexeme_buf, path, interner)
            .map(|(program, interner, arena)| ((program, arena), interner))
    })
    .map_err(|errs| {
        let msg = &errs[0].message;
        if label.is_empty() {
            msg.clone()
        } else {
            format!("{label}: {msg}")
        }
    })?;
    Ok((program, arena, interner))
}

/// Like [`parse_module`] but for callers that only walk the syntax (import
/// collection) and never lower it, so its `Atom`s never need to compare
/// against the resolver's shared table.
pub(crate) fn parse_only(
    source: &str,
    path: &str,
    label: &str,
) -> Result<(Program, AstArena, varn_core::AtomInterner), String> {
    let (tokens, lexeme_buf, _lex_errs) = varn_lexer::scan(source, path);
    // Unlike `parse_module`, this seeds a local `AtomInterner::new()` rather
    // than the resolver's shared snapshot, on purpose: the returned
    // `Atom`s/interner are used only within this function's caller to read
    // import specifiers back out as text, then dropped — they never escape
    // into a `Symbol`/`Type` that could later be compared against the
    // shared table, so there is nothing to keep in sync.
    varn_parser::parse(tokens, lexeme_buf, path, varn_core::AtomInterner::new())
        .map(|(program, interner, arena)| (program, arena, interner))
        .map_err(|errs| {
            let msg = &errs[0].message;
            if label.is_empty() {
                msg.clone()
            } else {
                format!("{label}: {msg}")
            }
        })
}
