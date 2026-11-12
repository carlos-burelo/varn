mod error;
mod expressions;
mod parser;
mod profile;
mod stream;
mod types;
use std::sync::Arc;

#[cfg(test)]
use vn_lexer as _;

pub use error::{PResult, ParseError};
pub use parser::Parser;
pub use profile::ParseProfile;
pub use stream::TokenStream;

use vn_core::{
    ast::{AstArena, Program},
    Token,
};

pub fn parse(
    tokens: Vec<Token>,
    lexeme_buf: Arc<[u8]>,
    filename: &str,
    interner: vn_core::AtomInterner,
) -> Result<(Program, vn_core::AtomInterner, AstArena), vn_core::DiagnosticBag> {
    let mut parser = Parser::new(tokens, lexeme_buf, Arc::from(filename), interner);
    let program = parser.parse_program()?;
    let arena = std::mem::take(&mut parser.stream.arena);
    Ok((program, parser.stream.interner, arena))
}

pub fn parse_with_profile(
    tokens: Vec<Token>,
    lexeme_buf: Arc<[u8]>,
    filename: &str,
    interner: vn_core::AtomInterner,
) -> Result<(Program, ParseProfile, vn_core::AtomInterner, AstArena), vn_core::DiagnosticBag> {
    let mut parser = Parser::new(tokens, lexeme_buf, Arc::from(filename), interner);
    let (program, profile) = parser.parse_program_with_profile()?;
    let arena = std::mem::take(&mut parser.stream.arena);
    Ok((program, profile, parser.stream.interner, arena))
}

pub fn parse_partial(
    tokens: Vec<Token>,
    lexeme_buf: Arc<[u8]>,
    filename: &str,
    interner: vn_core::AtomInterner,
) -> (
    Program,
    vn_core::DiagnosticBag,
    AstArena,
    vn_core::AtomInterner,
) {
    let mut parser = Parser::new(tokens, lexeme_buf, Arc::from(filename), interner);
    let (program, diagnostics) = parser.parse_program_partial();
    let arena = std::mem::take(&mut parser.stream.arena);
    (program, diagnostics, arena, parser.stream.interner)
}
