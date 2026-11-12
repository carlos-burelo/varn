use rustc_hash::FxHashMap;
use vn_core::TokenKind;

use crate::document::{DocumentState, TokenRecord};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatternRole {
    Variant,
    Binding,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemberRole {
    Method,
    Property,
}

pub struct Roles {
    pub patterns: FxHashMap<u32, PatternRole>,
    pub members: FxHashMap<u32, MemberRole>,
}

impl Roles {
    pub fn of(state: &DocumentState) -> Self {
        let tokens = &state.tokens;
        Self {
            patterns: pattern_roles(state, tokens),
            members: member_roles(tokens),
        }
    }
}

fn is_open(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::LParen | TokenKind::LBracket | TokenKind::LBrace
    )
}

fn is_close(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace
    )
}

fn pattern_roles(state: &DocumentState, tokens: &[TokenRecord]) -> FxHashMap<u32, PatternRole> {
    let mut roles = FxHashMap::default();
    for (index, token) in tokens.iter().enumerate() {
        if token.kind != TokenKind::Match {
            continue;
        }
        let Some(open_brace) = brace_after_scrutinee(tokens, index) else {
            continue;
        };
        let mut cursor = open_brace + 1;
        while cursor < tokens.len() && tokens[cursor].kind != TokenKind::RBrace {
            let Some(arrow) = find_arrow(tokens, cursor) else {
                break;
            };
            classify_pattern(state, tokens, cursor, arrow, &mut roles);
            cursor = skip_arm_body(tokens, arrow + 1);
            if tokens
                .get(cursor)
                .is_some_and(|t| t.kind == TokenKind::Comma)
            {
                cursor += 1;
            }
        }
    }
    roles
}

fn brace_after_scrutinee(tokens: &[TokenRecord], match_index: usize) -> Option<usize> {
    let open = match_index + 1;
    if tokens.get(open)?.kind != TokenKind::LParen {
        return None;
    }
    let mut depth = 0usize;
    for (offset, token) in tokens[open..].iter().enumerate() {
        if is_open(token.kind) {
            depth += 1;
        } else if is_close(token.kind) {
            depth -= 1;
            if depth == 0 {
                let after = open + offset + 1;
                return (tokens.get(after)?.kind == TokenKind::LBrace).then_some(after);
            }
        }
    }
    None
}

fn find_arrow(tokens: &[TokenRecord], from: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (offset, token) in tokens[from..].iter().enumerate() {
        if is_open(token.kind) {
            depth += 1;
        } else if is_close(token.kind) {
            if depth == 0 {
                return None;
            }
            depth -= 1;
        } else if depth == 0 && token.kind == TokenKind::FatArrow {
            return Some(from + offset);
        }
    }
    None
}

fn skip_arm_body(tokens: &[TokenRecord], from: usize) -> usize {
    let mut depth = 0usize;
    let mut index = from;
    let block_body = tokens.get(from).map(|t| t.kind) == Some(TokenKind::LBrace);
    while index < tokens.len() {
        let kind = tokens[index].kind;
        if is_open(kind) {
            depth += 1;
        } else if is_close(kind) {
            if depth == 0 {
                return index;
            }
            depth -= 1;
            if depth == 0 && block_body {
                return index + 1;
            }
        } else if depth == 0 && kind == TokenKind::Comma {
            return index;
        }
        index += 1;
    }
    index
}

fn classify_pattern(
    state: &DocumentState,
    tokens: &[TokenRecord],
    start: usize,
    arrow: usize,
    roles: &mut FxHashMap<u32, PatternRole>,
) {
    let mut depth = 0usize;
    for index in start..arrow {
        let token = &tokens[index];
        if is_open(token.kind) {
            depth += 1;
            continue;
        }
        if is_close(token.kind) {
            depth = depth.saturating_sub(1);
            continue;
        }
        if depth == 0 && token.kind == TokenKind::If {
            break;
        }
        if token.kind != TokenKind::Identifier {
            continue;
        }
        let lexeme = state.lexeme(token);
        let before = index.checked_sub(1).map(|k| tokens[k].kind);
        let after = tokens.get(index + 1).map(|t| t.kind);
        if lexeme == "_"
            || matches!(before, Some(TokenKind::Dot | TokenKind::ColonColon))
            || matches!(
                after,
                Some(TokenKind::Dot | TokenKind::ColonColon | TokenKind::Colon)
            )
        {
            continue;
        }
        let role = if after == Some(TokenKind::LParen) {
            PatternRole::Variant
        } else if depth > 0 || before == Some(TokenKind::DotDotDot) {
            PatternRole::Binding
        } else if lexeme.chars().next().is_some_and(char::is_uppercase) {
            PatternRole::Variant
        } else {
            PatternRole::Binding
        };
        roles.insert(token.offset, role);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Brace {
    TypeBody,
    Other,
}

fn member_roles(tokens: &[TokenRecord]) -> FxHashMap<u32, MemberRole> {
    let mut roles = FxHashMap::default();
    let mut stack: Vec<Brace> = Vec::new();
    let mut pending = false;
    for (index, token) in tokens.iter().enumerate() {
        let kind = token.kind;
        if matches!(
            kind,
            TokenKind::Class
                | TokenKind::Struct
                | TokenKind::Interface
                | TokenKind::Extension
                | TokenKind::Namespace
        ) {
            pending = true;
        } else if kind == TokenKind::LBrace {
            stack.push(if std::mem::take(&mut pending) {
                Brace::TypeBody
            } else {
                Brace::Other
            });
        } else if kind == TokenKind::RBrace {
            stack.pop();
        } else if kind == TokenKind::Semicolon {
            pending = false;
        } else if kind == TokenKind::Identifier
            && stack.last() == Some(&Brace::TypeBody)
            && at_member_slot(tokens, index)
        {
            let next = tokens.get(index + 1).map(|t| t.kind);
            let role = if matches!(next, Some(TokenKind::LParen | TokenKind::LAngle)) {
                MemberRole::Method
            } else {
                MemberRole::Property
            };
            roles.insert(token.offset, role);
        }
    }
    roles
}

fn at_member_slot(tokens: &[TokenRecord], index: usize) -> bool {
    let mut cursor = index;
    while cursor > 0 && is_member_prefix(tokens[cursor - 1].kind) {
        cursor -= 1;
    }
    cursor == 0
        || matches!(
            tokens[cursor - 1].kind,
            TokenKind::LBrace | TokenKind::RBrace | TokenKind::Semicolon | TokenKind::DocComment
        )
        || tokens[cursor - 1].line < tokens[cursor].line
}

fn is_member_prefix(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Public
            | TokenKind::Private
            | TokenKind::Protected
            | TokenKind::Static
            | TokenKind::Readonly
            | TokenKind::Abstract
            | TokenKind::Override
            | TokenKind::Async
            | TokenKind::Get
            | TokenKind::Set
            | TokenKind::Mut
            | TokenKind::Star
    )
}
