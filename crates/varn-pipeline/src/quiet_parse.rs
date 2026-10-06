use varn_core::ast::{AstArena, Program};

pub(crate) fn parse_module(
    source: &str,
    path: &str,
    label: &str,
) -> Result<(Program, AstArena, varn_core::AtomInterner), String> {
    let (tokens, lexeme_buf, _lex_errs) = varn_lexer::scan(source, path);
    let (program, interner, arena) =
        varn_parser::parse(tokens, lexeme_buf, path, varn_core::AtomInterner::new()).map_err(
            |errs| {
                let msg = &errs[0].message;
                if label.is_empty() {
                    msg.clone()
                } else {
                    format!("{label}: {msg}")
                }
            },
        )?;
    Ok((program, arena, interner))
}

pub(crate) fn parse_only(
    source: &str,
    path: &str,
    label: &str,
) -> Result<(Program, AstArena, varn_core::AtomInterner), String> {
    let (tokens, lexeme_buf, _lex_errs) = varn_lexer::scan(source, path);

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
