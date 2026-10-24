use varn_core::{DiagnosticBag, Trivia};

use crate::lex::Tokens;
use crate::parse::Parsed;
use crate::{Session, Source};

pub struct Analysis {
    pub tokens: Tokens,
    pub trivia: Vec<Trivia>,
    pub parsed: Parsed,
    pub checked: varn_sem::output::CheckResult,
    pub syntax: DiagnosticBag,
}

impl Session {
    pub fn analyze(&self, source: &Source) -> Analysis {
        let (tokens, trivia, mut syntax) = crate::lex::scan_tolerant(source.text(), source.path());
        let (parsed, syntactic) = crate::parse::parse_tolerant(
            tokens.tokens.clone(),
            tokens.lexeme_buf.clone(),
            source.path(),
        );
        syntax.extend(syntactic);
        let checked = crate::check::check_tolerant(
            &parsed.program,
            &parsed.arena,
            parsed.interner.clone(),
            self.resolver(),
        );
        Analysis {
            tokens,
            trivia,
            parsed,
            checked,
            syntax,
        }
    }
}
