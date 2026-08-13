use std::rc::Rc;
use varn_core::ast::Program;
use varn_core::{Diagnostic, Token};

pub struct SyntaxResult {
    pub raw_tokens: Vec<Token>,
    pub lexeme_buf: Rc<[u8]>,
    pub lex_errs: Vec<Diagnostic>,
    pub program: Program,
    pub parse_errs: Vec<Diagnostic>,
}

