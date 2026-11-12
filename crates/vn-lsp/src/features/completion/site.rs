use vn_core::TokenKind;
use vn_sem::symbol::SymbolKind;

use crate::document::{DocumentState, TokenRecord};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompletionSite {
    Statement,
    Expression,
    ClassBody,
    InterfaceBody,
    EnumBody,
    AfterExport,
    Type,
    NewTarget,
    ExtendsClass,
    ExtendsInterface,
    Implements,
    ObjectKeys {
        type_name: String,
        present: Vec<String>,
    },
}

pub fn completion_site(state: &DocumentState, cursor: u32) -> CompletionSite {
    let mut before: Vec<&TokenRecord> = state
        .tokens
        .iter()
        .filter(|t| t.offset < cursor && t.kind != TokenKind::EOF)
        .collect();
    if before
        .last()
        .is_some_and(|t| t.end >= cursor && is_word(t.kind))
    {
        before.pop();
    }
    if before.is_empty() {
        return CompletionSite::Statement;
    }
    classify(state, &before, before.len() - 1)
}

fn is_word(kind: TokenKind) -> bool {
    kind == TokenKind::Identifier || kind.is_keyword()
}

fn classify(state: &DocumentState, tokens: &[&TokenRecord], last: usize) -> CompletionSite {
    let kind = tokens[last].kind;
    if kind == TokenKind::New {
        return CompletionSite::NewTarget;
    }
    if matches!(kind, TokenKind::As | TokenKind::Is) {
        return CompletionSite::Type;
    }
    if matches!(
        kind,
        TokenKind::Extends | TokenKind::Implements | TokenKind::Comma
    ) && let Some(site) = heritage_site(tokens, last)
    {
        return site;
    }
    if kind == TokenKind::Colon {
        return after_colon(tokens, last);
    }
    if kind == TokenKind::Eq {
        return after_eq(tokens, last);
    }
    if kind == TokenKind::Comma
        && let Some(site) = object_keys(state, tokens, last)
    {
        return site;
    }
    if kind == TokenKind::Comma && type_body_kind(tokens, last + 1) == Some(BodyKind::Enum) {
        return CompletionSite::EnumBody;
    }
    if matches!(kind, TokenKind::LAngle | TokenKind::Lt | TokenKind::Comma) {
        return after_angle(state, tokens, last);
    }
    if matches!(kind, TokenKind::Pipe | TokenKind::Amp) {
        return after_union(state, tokens, last);
    }
    if kind == TokenKind::LBrace
        && let Some(site) = object_keys(state, tokens, last)
    {
        return site;
    }
    default_site(tokens, last)
}

fn heritage_site(tokens: &[&TokenRecord], last: usize) -> Option<CompletionSite> {
    let mut i = last;
    loop {
        let kind = tokens[i].kind;
        if kind == TokenKind::Implements {
            return Some(CompletionSite::Implements);
        }
        if kind == TokenKind::Extends {
            return extends_site(tokens, i);
        }
        let continues = matches!(
            kind,
            TokenKind::Identifier | TokenKind::Comma | TokenKind::Dot
        );
        if !continues || i == 0 {
            return None;
        }
        i -= 1;
    }
}

fn extends_site(tokens: &[&TokenRecord], extends: usize) -> Option<CompletionSite> {
    let mut angle_depth = 0usize;
    for token in tokens[..extends].iter().rev() {
        let kind = token.kind;
        if matches!(kind, TokenKind::RAngle | TokenKind::Gt) {
            angle_depth += 1;
        } else if matches!(kind, TokenKind::LAngle | TokenKind::Lt) {
            if angle_depth == 0 {
                return Some(CompletionSite::Type);
            }
            angle_depth -= 1;
        } else if angle_depth == 0 {
            if kind == TokenKind::Interface {
                return Some(CompletionSite::ExtendsInterface);
            }
            if matches!(kind, TokenKind::Class | TokenKind::Struct) {
                return Some(CompletionSite::ExtendsClass);
            }
            if matches!(
                kind,
                TokenKind::LBrace | TokenKind::RBrace | TokenKind::Semicolon
            ) {
                return None;
            }
        }
    }
    None
}

fn after_colon(tokens: &[&TokenRecord], colon: usize) -> CompletionSite {
    let Some(label) = colon.checked_sub(1) else {
        return CompletionSite::Expression;
    };
    let mut name = label;
    if tokens[name].kind == TokenKind::Question && name > 0 {
        name -= 1;
    }
    if tokens[name].kind == TokenKind::RParen {
        return if is_parameter_list_close(tokens, name) {
            CompletionSite::Type
        } else {
            CompletionSite::Expression
        };
    }
    if tokens[name].kind != TokenKind::Identifier && !tokens[name].kind.is_keyword() {
        return CompletionSite::Expression;
    }
    let binding = name
        .checked_sub(1)
        .map(|k| tokens[k].kind)
        .unwrap_or(TokenKind::EOF);
    if matches!(
        binding,
        TokenKind::Let | TokenKind::Const | TokenKind::Var | TokenKind::Using
    ) {
        return CompletionSite::Type;
    }
    if is_declaration_slot(tokens, name) {
        return CompletionSite::Type;
    }
    match innermost_open_paren(tokens, name) {
        Some(open) if is_parameter_list_open(tokens, open) => CompletionSite::Type,
        Some(_) | None => CompletionSite::Expression,
    }
}

fn is_declaration_slot(tokens: &[&TokenRecord], name: usize) -> bool {
    if !in_type_body(tokens, name) {
        return false;
    }
    let mut i = name;
    while i > 0 && is_modifier(tokens[i - 1].kind) {
        i -= 1;
    }
    i == 0
        || matches!(
            tokens[i - 1].kind,
            TokenKind::LBrace | TokenKind::RBrace | TokenKind::Semicolon | TokenKind::DocComment
        )
        || tokens[i - 1].line < tokens[name].line
}

fn is_modifier(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Public
            | TokenKind::Private
            | TokenKind::Protected
            | TokenKind::Static
            | TokenKind::Readonly
            | TokenKind::Abstract
            | TokenKind::Override
            | TokenKind::Declare
            | TokenKind::Mut
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BodyKind {
    Class,
    Interface,
    Enum,
}

fn type_body_kind(tokens: &[&TokenRecord], index: usize) -> Option<BodyKind> {
    let mut stack: Vec<Option<BodyKind>> = Vec::new();
    let mut pending = None;
    for token in &tokens[..index.min(tokens.len())] {
        let kind = token.kind;
        if matches!(kind, TokenKind::Class | TokenKind::Struct) {
            pending = Some(BodyKind::Class);
        } else if kind == TokenKind::Interface {
            pending = Some(BodyKind::Interface);
        } else if kind == TokenKind::Enum {
            pending = Some(BodyKind::Enum);
        } else if kind == TokenKind::LBrace {
            stack.push(pending.take());
        } else if kind == TokenKind::RBrace {
            stack.pop();
        } else if kind == TokenKind::Semicolon {
            pending = None;
        }
    }
    stack.last().copied().flatten()
}

fn in_type_body(tokens: &[&TokenRecord], index: usize) -> bool {
    matches!(
        type_body_kind(tokens, index),
        Some(BodyKind::Class | BodyKind::Interface)
    )
}

fn default_site(tokens: &[&TokenRecord], last: usize) -> CompletionSite {
    let kind = tokens[last].kind;
    if kind == TokenKind::Export {
        return CompletionSite::AfterExport;
    }
    let slot_start = matches!(
        kind,
        TokenKind::LBrace | TokenKind::RBrace | TokenKind::Semicolon | TokenKind::DocComment
    ) || is_modifier(kind)
        || matches!(kind, TokenKind::Async | TokenKind::Get | TokenKind::Set);
    match type_body_kind(tokens, last + 1) {
        Some(BodyKind::Class) if slot_start => return CompletionSite::ClassBody,
        Some(BodyKind::Interface) if slot_start => return CompletionSite::InterfaceBody,
        Some(BodyKind::Enum) if slot_start => return CompletionSite::EnumBody,
        Some(_) | None => {}
    }
    if matches!(
        kind,
        TokenKind::LBrace
            | TokenKind::RBrace
            | TokenKind::Semicolon
            | TokenKind::Else
            | TokenKind::Do
            | TokenKind::DocComment
    ) {
        return CompletionSite::Statement;
    }
    if kind == TokenKind::RParen && closes_control_header(tokens, last) {
        return CompletionSite::Statement;
    }
    CompletionSite::Expression
}

fn closes_control_header(tokens: &[&TokenRecord], close: usize) -> bool {
    let mut depth = 0usize;
    for i in (0..=close).rev() {
        let kind = tokens[i].kind;
        if kind == TokenKind::RParen {
            depth += 1;
        } else if kind == TokenKind::LParen {
            depth -= 1;
            if depth == 0 {
                return i.checked_sub(1).is_some_and(|k| {
                    matches!(
                        tokens[k].kind,
                        TokenKind::If | TokenKind::While | TokenKind::For | TokenKind::Catch
                    )
                });
            }
        }
    }
    false
}

fn innermost_open_paren(tokens: &[&TokenRecord], from: usize) -> Option<usize> {
    let mut depth = 0usize;
    for i in (0..from).rev() {
        let kind = tokens[i].kind;
        if kind == TokenKind::RParen {
            depth += 1;
        } else if kind == TokenKind::LParen {
            if depth == 0 {
                return Some(i);
            }
            depth -= 1;
        } else if matches!(kind, TokenKind::LBrace | TokenKind::Semicolon) && depth == 0 {
            return None;
        }
    }
    None
}

fn is_parameter_list_open(tokens: &[&TokenRecord], open: usize) -> bool {
    let Some(callee) = open.checked_sub(1) else {
        return false;
    };
    let kind = tokens[callee].kind;
    if kind == TokenKind::Constructor {
        return true;
    }
    if kind == TokenKind::Identifier || kind.is_keyword() {
        let before = callee.checked_sub(1).map(|k| tokens[k].kind);
        if before == Some(TokenKind::Function) {
            return true;
        }
        return in_type_body(tokens, callee) && is_declaration_slot_name(tokens, callee);
    }
    if kind == TokenKind::RAngle || kind == TokenKind::Gt {
        return generic_declaration_open(tokens, callee);
    }
    matches!(
        kind,
        TokenKind::Eq
            | TokenKind::LParen
            | TokenKind::Comma
            | TokenKind::Colon
            | TokenKind::Return
            | TokenKind::Async
            | TokenKind::FatArrow
    )
}

fn is_declaration_slot_name(tokens: &[&TokenRecord], name: usize) -> bool {
    let mut i = name;
    while i > 0 && (is_modifier(tokens[i - 1].kind) || tokens[i - 1].kind == TokenKind::Async) {
        i -= 1;
    }
    i == 0
        || matches!(
            tokens[i - 1].kind,
            TokenKind::LBrace | TokenKind::RBrace | TokenKind::Semicolon | TokenKind::DocComment
        )
        || tokens[i - 1].line < tokens[name].line
}

fn generic_declaration_open(tokens: &[&TokenRecord], close: usize) -> bool {
    let mut depth = 0usize;
    for i in (0..=close).rev() {
        let kind = tokens[i].kind;
        if matches!(kind, TokenKind::RAngle | TokenKind::Gt) {
            depth += 1;
        } else if matches!(kind, TokenKind::LAngle | TokenKind::Lt) {
            depth -= 1;
            if depth == 0 {
                return i
                    .checked_sub(2)
                    .is_some_and(|k| tokens[k].kind == TokenKind::Function);
            }
        }
    }
    false
}

fn is_parameter_list_close(tokens: &[&TokenRecord], close: usize) -> bool {
    let mut depth = 0usize;
    for i in (0..=close).rev() {
        let kind = tokens[i].kind;
        if kind == TokenKind::RParen {
            depth += 1;
        } else if kind == TokenKind::LParen {
            depth -= 1;
            if depth == 0 {
                return is_parameter_list_open(tokens, i);
            }
        }
    }
    false
}

fn after_eq(tokens: &[&TokenRecord], eq: usize) -> CompletionSite {
    let mut i = eq;
    if i == 0 {
        return CompletionSite::Expression;
    }
    i -= 1;
    if matches!(tokens[i].kind, TokenKind::RAngle | TokenKind::Gt) {
        let mut depth = 0usize;
        loop {
            let kind = tokens[i].kind;
            if matches!(kind, TokenKind::RAngle | TokenKind::Gt) {
                depth += 1;
            } else if matches!(kind, TokenKind::LAngle | TokenKind::Lt) {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            if i == 0 {
                return CompletionSite::Expression;
            }
            i -= 1;
        }
        if i == 0 {
            return CompletionSite::Expression;
        }
        i -= 1;
    }
    if tokens[i].kind == TokenKind::Identifier && i > 0 && tokens[i - 1].kind == TokenKind::Type {
        return CompletionSite::Type;
    }
    CompletionSite::Expression
}

fn after_angle(state: &DocumentState, tokens: &[&TokenRecord], last: usize) -> CompletionSite {
    let mut depth = 0usize;
    for i in (0..=last).rev() {
        let kind = tokens[i].kind;
        if matches!(kind, TokenKind::RAngle | TokenKind::Gt) {
            depth += 1;
        } else if matches!(kind, TokenKind::LAngle | TokenKind::Lt) {
            if depth == 0 {
                let Some(owner) = i.checked_sub(1) else {
                    return CompletionSite::Expression;
                };
                let name = state.lexeme(tokens[owner]);
                let names_a_type = matches!(
                    state.symbol_map.get(name),
                    Some(
                        SymbolKind::Class
                            | SymbolKind::Interface
                            | SymbolKind::TypeAlias
                            | SymbolKind::Enum
                    )
                );
                return if tokens[owner].kind == TokenKind::Identifier && names_a_type {
                    CompletionSite::Type
                } else {
                    CompletionSite::Expression
                };
            }
            depth -= 1;
        } else if matches!(
            kind,
            TokenKind::LBrace | TokenKind::RBrace | TokenKind::Semicolon | TokenKind::LParen
        ) {
            return CompletionSite::Expression;
        }
    }
    CompletionSite::Expression
}

fn after_union(state: &DocumentState, tokens: &[&TokenRecord], last: usize) -> CompletionSite {
    let mut i = last;
    while i > 0 {
        let kind = tokens[i - 1].kind;
        let operand = matches!(
            kind,
            TokenKind::Identifier
                | TokenKind::Pipe
                | TokenKind::Amp
                | TokenKind::Dot
                | TokenKind::LBracket
                | TokenKind::RBracket
                | TokenKind::Question
                | TokenKind::Null
                | TokenKind::Str
        );
        if !operand {
            break;
        }
        i -= 1;
    }
    if i == 0 {
        return CompletionSite::Expression;
    }
    if classify(state, tokens, i - 1) == CompletionSite::Type {
        CompletionSite::Type
    } else {
        CompletionSite::Expression
    }
}

fn object_keys(
    state: &DocumentState,
    tokens: &[&TokenRecord],
    last: usize,
) -> Option<CompletionSite> {
    let mut depth = 0usize;
    let mut open = None;
    for i in (0..=last).rev() {
        let kind = tokens[i].kind;
        if matches!(
            kind,
            TokenKind::RBrace | TokenKind::RParen | TokenKind::RBracket
        ) {
            depth += 1;
        } else if matches!(
            kind,
            TokenKind::LBrace | TokenKind::LParen | TokenKind::LBracket
        ) {
            if depth == 0 {
                if kind == TokenKind::LBrace {
                    open = Some(i);
                }
                break;
            }
            depth -= 1;
        }
    }
    let open = open?;
    if open < 3
        || tokens[open - 1].kind != TokenKind::Eq
        || tokens[open - 2].kind != TokenKind::Identifier
        || tokens[open - 3].kind != TokenKind::Colon
    {
        return None;
    }

    let mut present = Vec::new();
    let mut nested = 0usize;
    for i in open + 1..=last {
        let kind = tokens[i].kind;
        if matches!(
            kind,
            TokenKind::LBrace | TokenKind::LParen | TokenKind::LBracket
        ) {
            nested += 1;
        } else if matches!(
            kind,
            TokenKind::RBrace | TokenKind::RParen | TokenKind::RBracket
        ) {
            nested = nested.saturating_sub(1);
        } else if nested == 0
            && kind == TokenKind::Identifier
            && i < last
            && tokens[i + 1].kind == TokenKind::Colon
        {
            present.push(state.lexeme(tokens[i]).to_owned());
        }
    }
    Some(CompletionSite::ObjectKeys {
        type_name: state.lexeme(tokens[open - 2]).to_owned(),
        present,
    })
}
