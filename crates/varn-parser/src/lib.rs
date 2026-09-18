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

/// `interner` is owned by the caller and comes back enlarged in the returned
/// tuple: pass `varn_core::AtomInterner::new()` for a self-contained parse, or
/// a clone of a compilation-wide table (see `DiskResolver::interner_snapshot`)
/// when this file's `Atom`s must compare equal to another module's already
/// parsed in the same compilation.
pub fn parse(
    tokens: Vec<Token>,
    lexeme_buf: Rc<[u8]>,
    filename: &str,
    interner: varn_core::AtomInterner,
) -> Result<(Program, varn_core::AtomInterner), varn_core::DiagnosticBag> {
    let mut parser = Parser::new(tokens, lexeme_buf, Rc::from(filename), interner);
    let program = parser.parse_program()?;
    Ok((program, parser.stream.interner))
}

pub fn parse_with_profile(
    tokens: Vec<Token>,
    lexeme_buf: Rc<[u8]>,
    filename: &str,
    interner: varn_core::AtomInterner,
) -> Result<(Program, ParseProfile, varn_core::AtomInterner), varn_core::DiagnosticBag> {
    let mut parser = Parser::new(tokens, lexeme_buf, Rc::from(filename), interner);
    let (program, profile) = parser.parse_program_with_profile()?;
    Ok((program, profile, parser.stream.interner))
}

/// Never shares state with another parse: partial parses back the LSP's
/// incremental single-file path, which does not carry an `Atom` table forward
/// between edits.
pub fn parse_partial(
    tokens: Vec<Token>,
    lexeme_buf: Rc<[u8]>,
    filename: &str,
) -> (Program, varn_core::DiagnosticBag) {
    let mut parser = Parser::new(
        tokens,
        lexeme_buf,
        Rc::from(filename),
        varn_core::AtomInterner::new(),
    );
    parser.parse_program_partial()
}
