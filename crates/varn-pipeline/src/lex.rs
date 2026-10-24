use crate::Failure;
use std::sync::Arc;
use varn_core::Token;

pub struct Tokens {
    pub tokens: Vec<Token>,
    pub lexeme_buf: Arc<[u8]>,
}

pub fn scan(source: &str, path: &str) -> Result<Tokens, Failure> {
    let (tokens, lexeme_buf, errors) = varn_lexer::scan(source, path);

    if !errors.is_empty() {
        let mut bag = varn_core::DiagnosticBag::new();
        for e in errors {
            bag.push(e);
        }
        return Err(Failure::diagnostics(path, source, bag));
    }

    Ok(Tokens { tokens, lexeme_buf })
}
