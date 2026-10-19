use varn_core::ast::{AstArena, Program};

pub(crate) fn parse_module(
    source: &str,
    path: &str,
    label: &str,
) -> Result<(Program, AstArena, varn_core::AtomInterner), String> {
    let labeled = |msg: &str| {
        if label.is_empty() {
            msg.to_owned()
        } else {
            format!("{label}: {msg}")
        }
    };
    let (tokens, lexeme_buf, lex_errs) = varn_lexer::scan(source, path);
    if let Some(first) = lex_errs.first() {
        return Err(labeled(&first.message));
    }
    let (program, interner, arena) =
        varn_parser::parse(tokens, lexeme_buf, path, varn_core::AtomInterner::new())
            .map_err(|errs| labeled(&errs[0].message))?;
    Ok((program, arena, interner))
}
