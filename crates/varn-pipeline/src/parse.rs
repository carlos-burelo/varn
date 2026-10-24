use crate::Failure;
use std::sync::Arc;
use varn_core::ast::{AstArena, Program};
use varn_core::{AtomInterner, Token};

pub struct Parsed {
    pub program: Program,
    pub arena: AstArena,
    pub interner: AtomInterner,
}

pub(crate) fn parse(
    tokens: Vec<Token>,
    lexeme_buf: Arc<[u8]>,
    source: &str,
    path: &str,
) -> Result<Parsed, Failure> {
    let (program, interner, arena) = varn_parser::parse(tokens, lexeme_buf, path, AtomInterner::new())
        .map_err(|errs| {
            let mut bag = varn_core::DiagnosticBag::new();
            for e in errs {
                bag.push(e);
            }
            Failure::diagnostics(path, source, bag)
        })?;

    Ok(Parsed {
        program,
        arena,
        interner,
    })
}
