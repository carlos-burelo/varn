use crate::stream::TokenStream;
use varn_core::ast::{ExprId, ExprKind};
use varn_core::ErrorCode;

pub(super) fn parse_regex_literal(s: &mut TokenStream) -> ExprId {
    let range = s.range();
    let raw = s.consume_lexeme();
    let text = s.interner.resolve(raw).to_owned();
    let closing = text.rfind('/').filter(|&at| at > 0);
    let outcome = match closing {
        Some(at) => {
            let (pattern, flags) = (&text[1..at], &text[at + 1..]);
            varn_core::regex_pattern::compile(pattern, flags)
                .map(|_| (pattern.to_owned(), flags.to_owned()))
                .map_err(|e| e.to_string())
        }
        None => Err("unterminated regular expression".to_owned()),
    };
    match outcome {
        Ok((pattern, flags)) => {
            let binding = s.regex_binding(&pattern, &flags, range);
            s.expr(range, ExprKind::Identifier { name: binding })
        }
        Err(message) => {
            let diagnostic = varn_core::Diagnostic::error(ErrorCode::InvalidRegex, message)
                .with_range(range)
                .with_file(s.filename.clone());
            s.errors.push(diagnostic);
            s.expr(range, ExprKind::Missing)
        }
    }
}
