//! Lex + parse for the internal compiles that report through `Result`.
//!
//! The `lex`/`parse` phases render diagnostics to the terminal, honour
//! `--verbose` and feed the `-p tokens` dump -- correct for a user-invoked run,
//! wrong for stdlib bundling and module precompilation, which must stay silent
//! and hand a message back to their caller. This is the driver for those.

use varn_core::ast::Program;

pub(crate) fn parse_module(
    source: &str,
    path: &str,
    label: &str,
) -> Result<(Program, varn_core::AtomInterner), String> {
    let (tokens, lexeme_buf, _lex_errs) = varn_lexer::scan(source, path);
    // Same fix as `parse::parse`: a fresh `AtomInterner::new()` per module made
    // this module's `Atom`s incomparable with the resolver's shared table, so
    // an imported symbol's `origin_module` (set from another module's atoms in
    // `binder/imports.rs`) could never resolve back to this module's own
    // atoms. Seed from the shared snapshot, publish back only on success so a
    // failed parse of this module doesn't lose atoms already coined elsewhere.
    let interner = crate::resolver::with_resolver(|r| r.interner_snapshot());
    // TODO(fase1-componente2): this function's own public signature is still
    // `(Program, AtomInterner)`; the arena is dropped here until varn-pipeline
    // migrates in a later task.
    let (program, interner, _arena) =
        varn_parser::parse(tokens, lexeme_buf, path, interner).map_err(|errs| {
            let msg = &errs[0].message;
            if label.is_empty() {
                msg.clone()
            } else {
                format!("{label}: {msg}")
            }
        })?;
    crate::resolver::with_resolver(|r| r.set_interner(interner.clone()));
    Ok((program, interner))
}

/// Like [`parse_module`] but without assigning AST ids, for callers that only
/// walk the syntax (import collection) and never lower it.
pub(crate) fn parse_only(
    source: &str,
    path: &str,
    label: &str,
) -> Result<(Program, varn_core::AtomInterner), String> {
    let (tokens, lexeme_buf, _lex_errs) = varn_lexer::scan(source, path);
    // TODO(fase1-componente2): same deferred arena as `parse_module` above.
    varn_parser::parse(tokens, lexeme_buf, path, varn_core::AtomInterner::new())
        .map(|(program, interner, _arena)| (program, interner))
        .map_err(|errs| {
            let msg = &errs[0].message;
            if label.is_empty() {
                msg.clone()
            } else {
                format!("{label}: {msg}")
            }
        })
}
