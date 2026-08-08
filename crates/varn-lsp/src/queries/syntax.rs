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

pub fn parse_syntax(source: &str, path: &str) -> SyntaxResult {
    let (raw_tokens, lexeme_buf, lex_errs) = varn_lexer::scan(source, path);
    let (mut program, parse_errs) =
        varn_parser::parse_partial(raw_tokens.clone(), lexeme_buf.clone(), path);
    varn_core::assign_ast_ids(&mut program);

    SyntaxResult {
        raw_tokens,
        lexeme_buf,
        lex_errs,
        program,
        parse_errs: parse_errs.into_iter().collect(),
    }
}
