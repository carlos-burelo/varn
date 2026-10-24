use crate::Failure;
use std::sync::Arc;
use varn_core::{DiagnosticBag, Token, Trivia};

pub struct Tokens {
    pub tokens: Vec<Token>,
    pub lexeme_buf: Arc<[u8]>,
    pub trivia: Vec<Trivia>,
}

pub(crate) fn scan(source: &str, path: &str) -> Result<Tokens, Failure> {
    let (tokens, lexeme_buf, errors) = varn_lexer::scan(source, path);

    if !errors.is_empty() {
        let mut bag = varn_core::DiagnosticBag::new();
        for e in errors {
            bag.push(e);
        }
        return Err(Failure::diagnostics(path, source, bag));
    }

    Ok(Tokens {
        tokens,
        lexeme_buf,
        trivia: Vec::new(),
    })
}

pub(crate) fn scan_tolerant(source: &str, path: &str) -> (Tokens, DiagnosticBag) {
    let (tokens, lexeme_buf, errors, trivia) = varn_lexer::scan_with_trivia(source, path);
    let mut diagnostics = DiagnosticBag::new();
    for e in errors {
        diagnostics.push(e);
    }
    (
        Tokens {
            tokens,
            lexeme_buf,
            trivia,
        },
        diagnostics,
    )
}
