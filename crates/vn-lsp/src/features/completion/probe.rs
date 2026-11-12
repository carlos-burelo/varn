use crate::document::{DocumentState, TokenRecord};
use vn_core::TokenKind;
use vn_sem::symbol::SymbolKind;

pub const PROBE_NAME: &str = "__vn_probe__";
const PROBE_DECLARATION: &str = "function __vn_probe__<T>(value: T): T { return value } ";
pub const MEMBER_PLACEHOLDER: &str = "__vn_member__";

pub struct ProbedSource {
    pub text: String,
    pub line: u32,
    pub character: u32,
}

pub fn probed_source(state: &DocumentState, cursor: u32) -> Option<ProbedSource> {
    let tokens = &state.tokens;
    let dot = tokens.iter().rposition(|t| {
        matches!(t.kind, TokenKind::Dot | TokenKind::QuestionDot) && t.offset < cursor
    })?;
    if tokens[dot + 1..]
        .iter()
        .take_while(|t| t.offset < cursor)
        .any(|t| t.kind != TokenKind::Identifier && !t.kind.can_be_identifier())
    {
        return None;
    }
    let tail_start = tokens
        .get(dot + 1)
        .filter(|t| t.offset < cursor)
        .map_or(cursor, |t| t.offset);
    if state
        .source
        .get(cursor as usize..)?
        .chars()
        .next()
        .is_some_and(|c| c.is_alphanumeric() || c == '_')
    {
        return None;
    }

    let first = receiver_start(tokens, dot)?;
    let receiver_text = state
        .source
        .get(tokens[first].offset as usize..tokens[dot].offset as usize)?
        .trim();
    if receiver_text.is_empty() {
        return None;
    }
    if first + 1 == dot
        && matches!(
            state.symbol_map.get(state.lexeme(&tokens[first])),
            Some(
                SymbolKind::Class
                    | SymbolKind::Interface
                    | SymbolKind::Enum
                    | SymbolKind::Namespace
            )
        )
    {
        return None;
    }
    let receiver_start = tokens[first].offset as usize;
    let receiver_end = tokens[dot].offset as usize;
    let cursor = cursor as usize;

    let mut text = String::with_capacity(state.source.len() + PROBE_DECLARATION.len() + 32);
    text.push_str(PROBE_DECLARATION);
    text.push_str(state.source.get(..receiver_start)?);
    text.push_str(PROBE_NAME);
    text.push('(');
    text.push_str(state.source.get(receiver_start..receiver_end)?.trim_end());
    text.push(')');
    text.push_str(state.source.get(receiver_end..tail_start as usize)?);
    let new_cursor = text.len();
    text.push_str(MEMBER_PLACEHOLDER);
    text.push_str(state.source.get(cursor..)?);

    let before = &text[..new_cursor];
    let line = before.matches('\n').count() as u32;
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    let character = before[line_start..].encode_utf16().count() as u32;
    Some(ProbedSource {
        text,
        line,
        character,
    })
}

fn matching_open(tokens: &[TokenRecord], close: usize) -> Option<usize> {
    let mut depth = 0usize;
    for i in (0..=close).rev() {
        let kind = tokens[i].kind;
        if matches!(
            kind,
            TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace
        ) {
            depth += 1;
        } else if matches!(
            kind,
            TokenKind::LParen | TokenKind::LBracket | TokenKind::LBrace
        ) {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
    }
    None
}

fn receiver_start(tokens: &[TokenRecord], dot: usize) -> Option<usize> {
    let mut end = dot;
    loop {
        let last = end.checked_sub(1)?;
        let kind = tokens[last].kind;
        if kind == TokenKind::Bang {
            end = last;
            continue;
        }
        if matches!(kind, TokenKind::RParen | TokenKind::RBracket) {
            let open = matching_open(tokens, last)?;
            let callee = open.checked_sub(1).map(|k| tokens[k].kind);
            let continues = callee.is_some_and(|k| {
                matches!(
                    k,
                    TokenKind::Identifier
                        | TokenKind::RParen
                        | TokenKind::RBracket
                        | TokenKind::RAngle
                        | TokenKind::Gt
                        | TokenKind::Bang
                )
            });
            if continues {
                end = open;
                continue;
            }
            return Some(open);
        }
        let primary = kind == TokenKind::Identifier
            || kind.can_be_identifier()
            || kind.is_literal()
            || matches!(
                kind,
                TokenKind::This
                    | TokenKind::Super
                    | TokenKind::Template
                    | TokenKind::RegularExpression
            );
        if !primary {
            return None;
        }
        let before = last.checked_sub(1).map(|k| tokens[k].kind);
        if matches!(before, Some(TokenKind::Dot | TokenKind::QuestionDot)) {
            end = last - 1;
            continue;
        }
        if before == Some(TokenKind::New) {
            return Some(last - 1);
        }
        return Some(last);
    }
}
