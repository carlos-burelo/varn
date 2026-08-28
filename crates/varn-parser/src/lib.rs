mod expressions;
mod parser;
mod profile;
mod stream;
mod types;
use std::rc::Rc;

#[cfg(test)]
use varn_lexer as _;

pub use parser::Parser;
pub use profile::ParseProfile;
pub use stream::TokenStream;

use varn_core::{ast::Program, Token};

pub fn parse(
    tokens: Vec<Token>,
    lexeme_buf: Rc<[u8]>,
    filename: &str,
) -> Result<Program, varn_core::DiagnosticBag> {
    let mut parser = Parser::new(tokens, lexeme_buf, Rc::from(filename));
    parser.parse_program()
}

pub fn parse_with_profile(
    tokens: Vec<Token>,
    lexeme_buf: Rc<[u8]>,
    filename: &str,
) -> Result<(Program, ParseProfile), varn_core::DiagnosticBag> {
    let mut parser = Parser::new(tokens, lexeme_buf, Rc::from(filename));
    parser.parse_program_with_profile()
}

pub fn parse_partial(
    tokens: Vec<Token>,
    lexeme_buf: Rc<[u8]>,
    filename: &str,
) -> (Program, varn_core::DiagnosticBag) {
    let mut parser = Parser::new(tokens, lexeme_buf, Rc::from(filename));
    parser.parse_program_partial()
}
