use crate::scanner::Scanner;

pub fn scan(
    source: &str,
    filename: &str,
) -> (
    Vec<varn_core::Token>,
    std::rc::Rc<[u8]>,
    Vec<varn_core::Diagnostic>,
) {
    let (tokens, buf, diags, _) =
        scan_inner(source, filename, crate::scanner::LexerConfig::default());
    (tokens, buf, diags)
}

/// [`scan`] plus the comments it would otherwise drop.
///
/// Separate entry point rather than a fourth element on `scan`: only tooling
/// that reproduces source needs trivia, and every other caller — the whole
/// compile path — would pay the churn for a value it discards.
pub fn scan_with_trivia(
    source: &str,
    filename: &str,
) -> (
    Vec<varn_core::Token>,
    std::rc::Rc<[u8]>,
    Vec<varn_core::Diagnostic>,
    Vec<varn_core::Trivia>,
) {
    scan_inner(
        source,
        filename,
        crate::scanner::LexerConfig {
            emit_trivia: true,
            ..Default::default()
        },
    )
}

fn parse_num_lexeme(kind: varn_core::TokenKind, lexeme: &str) -> Option<varn_core::ParsedNumber> {
    use varn_core::{ParsedNumber, TokenKind};
    let cleaned = lexeme.replace('_', "");
    match kind {
        TokenKind::IntegerLiteral => cleaned.parse::<i64>().ok().map(ParsedNumber::Int),
        TokenKind::BinaryLiteral => {
            let digits = cleaned
                .strip_prefix("0b")
                .or_else(|| cleaned.strip_prefix("0B"))?;
            i64::from_str_radix(digits, 2).ok().map(ParsedNumber::Int)
        }
        TokenKind::OctalLiteral => {
            let digits = cleaned
                .strip_prefix("0o")
                .or_else(|| cleaned.strip_prefix("0O"))?;
            i64::from_str_radix(digits, 8).ok().map(ParsedNumber::Int)
        }
        TokenKind::HexLiteral => {
            let digits = cleaned
                .strip_prefix("0x")
                .or_else(|| cleaned.strip_prefix("0X"))?;
            i64::from_str_radix(digits, 16).ok().map(ParsedNumber::Int)
        }
        TokenKind::FloatLiteral => cleaned.parse::<f64>().ok().map(ParsedNumber::Float),
        _ => None,
    }
}

pub fn scan_with_config(
    source: &str,
    filename: &str,
    config: crate::scanner::LexerConfig,
) -> (
    Vec<varn_core::Token>,
    std::rc::Rc<[u8]>,
    Vec<varn_core::Diagnostic>,
) {
    let (tokens, buf, diags, _) = scan_inner(source, filename, config);
    (tokens, buf, diags)
}

fn scan_inner(
    source: &str,
    _filename: &str,
    config: crate::scanner::LexerConfig,
) -> (
    Vec<varn_core::Token>,
    std::rc::Rc<[u8]>,
    Vec<varn_core::Diagnostic>,
    Vec<varn_core::Trivia>,
) {
    use varn_core::{SourceLocation, SourceRange, Token, TokenKind};

    let src_bytes = source.as_bytes();

    let mut scanner = Scanner::with_config(src_bytes, config);
    let (records, lexeme_buf) = scanner.scan_all();
    let diagnostics = scanner.diagnostics.take();
    let trivia = std::mem::take(&mut scanner.trivia);

    let tokens: Vec<varn_core::Token> = records
        .into_iter()
        .map(|r| {
            let lexeme_str = std::str::from_utf8(
                &lexeme_buf[r.lex_offset as usize..(r.lex_offset + r.lex_len) as usize],
            )
            .unwrap_or("");

            let start = SourceLocation {
                line: r.start_line,
                column: r.start_col,
                offset: r.start_offset,
            };
            let end = SourceLocation {
                line: r.end_line,
                column: r.end_col,
                offset: r.end_offset,
            };
            let range = SourceRange { start, end };

            let kind = TokenKind::from_u32(r.kind);
            let parsed_num = parse_num_lexeme(kind, lexeme_str);

            Token {
                kind,
                range,
                parsed_num,
                lex_start: r.lex_offset,
                lex_len: r.lex_len,
            }
        })
        .collect();

    let rc_buf: std::rc::Rc<[u8]> = lexeme_buf.into();
    (tokens, rc_buf, diagnostics, trivia)
}
