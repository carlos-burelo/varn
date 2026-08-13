//! Lex + parse for the internal compiles that report through `Result`.
//!
//! The `lex`/`parse` phases render diagnostics to the terminal, honour
//! `--verbose` and feed the `-p tokens` dump -- correct for a user-invoked run,
//! wrong for stdlib bundling and module precompilation, which must stay silent
//! and hand a message back to their caller. This is the driver for those.

use varn_core::ast::Program;

/// Parse `source` and assign AST ids, mapping the first parse error to a
/// message with `label` as its prefix (empty label = the bare message).
pub(crate) fn parse_module(source: &str, path: &str, label: &str) -> Result<Program, String> {
    let mut program = parse_only(source, path, label)?;
    varn_core::assign_ast_ids(&mut program);
    Ok(program)
}

/// Like [`parse_module`] but without assigning AST ids, for callers that only
/// walk the syntax (import collection) and never lower it.
pub(crate) fn parse_only(source: &str, path: &str, label: &str) -> Result<Program, String> {
    let (tokens, lexeme_buf, _lex_errs) = varn_lexer::scan(source, path);
    varn_parser::parse(tokens, lexeme_buf, path).map_err(|errs| {
        let msg = &errs[0].message;
        if label.is_empty() {
            msg.clone()
        } else {
            format!("{label}: {msg}")
        }
    })
}
