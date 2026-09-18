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
    // Same fix as `parse::parse`: a fresh `AtomInterner::new()` per module made
    // this module's `Atom`s incomparable with the resolver's shared table, so
    // an imported symbol's `origin_module` (set from another module's atoms in
    // `binder/imports.rs`) could never resolve back to this module's own
    // atoms. Seed from the shared snapshot, publish back only on success so a
    // failed parse of this module doesn't lose atoms already coined elsewhere.
    let interner = crate::resolver::with_resolver(|r| r.interner_snapshot());
    let (program, interner, arena) =
        varn_parser::parse(tokens, lexeme_buf, path, interner).map_err(|errs| {
            let msg = &errs[0].message;
            if label.is_empty() {
                msg.clone()
            } else {
                format!("{label}: {msg}")
            }
        })?;
    crate::resolver::with_resolver(|r| r.set_interner(interner.clone()));
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
