use varn_core::ast::{AstArena, Program};

pub(crate) fn parse_module(
    source: &str,
    path: &str,
) -> Result<(Program, AstArena, varn_core::AtomInterner), String> {
    let (tokens, lexeme_buf, lex_errs) = varn_lexer::scan(source, path);
    if let Some(first) = lex_errs.first() {
        return Err(format!("{path}: {}", first.message));
    }
    varn_parser::parse(tokens, lexeme_buf, path, varn_core::AtomInterner::new())
        .map(|(program, interner, arena)| (program, arena, interner))
        .map_err(|errs| format!("{path}: {}", errs[0].message))
}
