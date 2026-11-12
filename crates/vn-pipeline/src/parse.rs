use std::sync::Arc;
use vn_core::ast::{AstArena, Program};
use vn_core::{AtomInterner, Token};

pub struct Parsed {
    pub program: Program,
    pub arena: AstArena,
    pub interner: AtomInterner,
}

pub(crate) fn parse_tolerant(
    tokens: Vec<Token>,
    lexeme_buf: Arc<[u8]>,
    path: &str,
) -> (Parsed, vn_core::DiagnosticBag) {
    let (program, diagnostics, arena, interner) =
        vn_parser::parse_partial(tokens, lexeme_buf, path, AtomInterner::new());
    (
        Parsed {
            program,
            arena,
            interner,
        },
        diagnostics,
    )
}
