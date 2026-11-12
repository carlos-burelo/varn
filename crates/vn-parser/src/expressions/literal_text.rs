use crate::error::PResult;
use crate::stream::TokenStream;
use vn_core::ast::{operators::UnaryOp, ExprId, ExprKind};
use vn_core::{ErrorCode, SourceRange, TokenKind};

pub(crate) fn parse_literal_atom(s: &mut TokenStream) -> PResult<ExprId> {
    let start = s.range();
    if !s.eat(TokenKind::Minus) {
        return super::primary::parse_primary_expr(s);
    }
    if let Some(expr) = take_min_integer(s, start) {
        return Ok(expr);
    }
    let operand = super::primary::parse_primary_expr(s)?;
    let range = s.span_from(start);
    Ok(s.expr(
        range,
        ExprKind::Unary {
            op: UnaryOp::Minus,
            prefix: true,
            operand,
        },
    ))
}

pub(crate) fn take_min_integer(s: &mut TokenStream, start: SourceRange) -> Option<ExprId> {
    const I64_MIN_MAGNITUDE: &str = "9223372036854775808";
    if s.kind() != TokenKind::IntegerLiteral || s.lexeme().replace('_', "") != I64_MIN_MAGNITUDE {
        return None;
    }
    s.advance();
    let range = s.span_from(start);
    let raw = s.interner.intern(&format!("-{I64_MIN_MAGNITUDE}"));
    Some(s.expr(
        range,
        ExprKind::IntLiteral {
            value: i64::MIN,
            raw,
        },
    ))
}

pub(super) fn parse_int_radix(raw: &str) -> Result<i64, std::num::IntErrorKind> {
    let cleaned = raw.replace('_', "");
    let (digits, radix) = if let Some(hex) = cleaned
        .strip_prefix("0x")
        .or_else(|| cleaned.strip_prefix("0X"))
    {
        (hex, 16)
    } else if let Some(bin) = cleaned
        .strip_prefix("0b")
        .or_else(|| cleaned.strip_prefix("0B"))
    {
        (bin, 2)
    } else if let Some(oct) = cleaned
        .strip_prefix("0o")
        .or_else(|| cleaned.strip_prefix("0O"))
    {
        (oct, 8)
    } else {
        (cleaned.as_str(), 10)
    };
    i64::from_str_radix(digits, radix).map_err(|error| *error.kind())
}

pub(crate) struct EscapeIssue {
    pub code: ErrorCode,
    pub message: String,
    pub at: usize,
    pub len: usize,
}

pub(crate) fn unescape_string(raw: &str) -> (String, Vec<EscapeIssue>) {
    let mut result = String::with_capacity(raw.len());
    let mut issues = Vec::new();
    let mut chars = raw.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        if c != '\\' {
            result.push(c);
            continue;
        }
        let Some((_, escape)) = chars.next() else {
            issues.push(EscapeIssue {
                code: ErrorCode::InvalidEscapeSequence,
                message: "a backslash must be followed by the character it escapes".to_owned(),
                at,
                len: 1,
            });
            continue;
        };
        match escape {
            'n' => result.push('\n'),
            't' => result.push('\t'),
            'r' => result.push('\r'),
            '\\' => result.push('\\'),
            '\'' => result.push('\''),
            '"' => result.push('"'),
            '0' => result.push('\0'),
            'u' => {
                let braced = chars.peek().is_some_and(|(_, next)| *next == '{');
                let (digits, consumed) = if braced {
                    chars.next();
                    let mut digits = String::new();
                    let mut closed = false;
                    for (_, next) in chars.by_ref() {
                        if next == '}' {
                            closed = true;
                            break;
                        }
                        digits.push(next);
                    }
                    (digits, 2 + usize::from(closed) + 1)
                } else {
                    let digits: String = chars.by_ref().take(4).map(|(_, d)| d).collect();
                    (digits, 2)
                };
                let decoded = if braced || digits.len() == 4 {
                    u32::from_str_radix(&digits, 16)
                        .ok()
                        .and_then(char::from_u32)
                } else {
                    None
                };
                match decoded {
                    Some(ch) => result.push(ch),
                    None => {
                        let written = if braced {
                            format!("{{{digits}}}")
                        } else {
                            digits.clone()
                        };
                        issues.push(EscapeIssue {
                            code: ErrorCode::InvalidUnicodeEscape,
                            message: format!(
                                "`\\u{written}` is not a valid unicode escape: write `\\u{{XXXX}}` with a Unicode scalar value"
                            ),
                            at,
                            len: consumed + digits.len(),
                        });
                    }
                }
            }
            'x' => {
                let digits: String = chars.by_ref().take(2).map(|(_, d)| d).collect();
                match u8::from_str_radix(&digits, 16) {
                    Ok(n) if digits.len() == 2 => result.push(n as char),
                    Ok(_) | Err(_) => issues.push(EscapeIssue {
                        code: ErrorCode::InvalidEscapeSequence,
                        message: format!(
                            "`\\x{digits}` is not a valid escape: write `\\xHH` with two hex digits"
                        ),
                        at,
                        len: 2 + digits.len(),
                    }),
                }
            }
            other => issues.push(EscapeIssue {
                code: ErrorCode::InvalidEscapeSequence,
                message: format!("unknown escape sequence `\\{other}`"),
                at,
                len: 1 + other.len_utf8(),
            }),
        }
    }
    (result, issues)
}

pub(crate) fn unescape_reporting(
    s: &mut TokenStream,
    raw: &str,
    token: SourceRange,
    base_offset: u32,
) -> String {
    let (value, issues) = unescape_string(raw);
    for issue in issues {
        let delta = base_offset.saturating_sub(token.start.offset) + issue.at as u32;
        let mut start = token.start;
        start.offset = token.start.offset + delta;
        start.column = token.start.column + delta;
        let mut end = start;
        end.offset += issue.len as u32;
        end.column += issue.len as u32;
        s.push_diagnostic(issue.code, issue.message, SourceRange { start, end });
    }
    value
}
