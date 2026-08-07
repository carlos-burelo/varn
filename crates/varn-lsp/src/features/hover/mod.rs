mod format;
mod imports;
mod members;
mod symbols;

pub use members::{format_enum_member, format_member_sig};
pub use symbols::symbol_hover;

use tower_lsp::lsp_types::{Hover, HoverContents, MarkupContent, MarkupKind};
use varn_checker::SymbolKind;
use varn_core::TokenKind;

use crate::document::{ChainResult, DocumentState, MemberKind};
use crate::query;

pub fn build_hover(state: &DocumentState, line: u32, col: u32) -> Option<Hover> {
    if let Some(ctx) = query::import_path_at(&state.source, line, col) {
        return imports::import_path_hover(&ctx.specifier, &state.uri);
    }

    let tok_any = state
        .tokens
        .iter()
        .enumerate()
        .find(|(_, t)| t.line == line && t.col <= col && col < t.col + t.length);
    if let Some((idx, tok)) = tok_any {
        if matches!(
            tok.kind,
            TokenKind::As | TokenKind::Import | TokenKind::Export
        ) {
            return None;
        }
        if tok.kind == TokenKind::This {
            let enclosing = state
                .symbols
                .iter()
                .filter(|s| {
                    !s.is_from_stdlib
                        && matches!(s.kind, SymbolKind::Class | SymbolKind::Interface)
                        && s.line <= line
                })
                .max_by_key(|s| s.line);
            if let Some(cls) = enclosing {
                return Some(make_lang_hover(format!("this: {}", cls.name)));
            }
            return Some(make_lang_hover("this".to_owned()));
        }

        let prev_is_dot = idx
            .checked_sub(1)
            .and_then(|j| state.tokens.get(j))
            .map(|t| t.kind == TokenKind::Dot)
            .unwrap_or(false);
        if tok.kind.is_keyword() && !prev_is_dot {
            return None;
        }
    }

    if let Some(res) = query::resolve_chain(state, line, col) {
        match res {
            ChainResult::Symbol(sym) => return Some(symbol_hover(sym)),
            ChainResult::Member {
                member,
                parent_name,
            } => {
                let sig = if member.kind == MemberKind::EnumMember {
                    format_enum_member(&parent_name, &member.name, &member.init_value)
                } else {
                    format_member_sig(&parent_name, member)
                };
                return Some(make_lang_hover(sig));
            }
            ChainResult::DynamicMember {
                member,
                parent_name,
            } => {
                let sig = if member.kind == MemberKind::EnumMember {
                    format_enum_member(&parent_name, &member.name, &member.init_value)
                } else {
                    format_member_sig(&parent_name, &member)
                };
                return Some(make_lang_hover(sig));
            }
        }
    }

    if let Some(sym) = query::symbol_at(state, line, col) {
        return Some(symbol_hover(sym));
    }

    if let Some((parent_name, parent_kind, member)) = query::member_at(state, line, col) {
        let sig = if parent_kind == SymbolKind::Enum && member.kind == MemberKind::EnumMember {
            format_enum_member(&parent_name, &member.name, &member.init_value)
        } else {
            format_member_sig(&parent_name, member)
        };
        return Some(make_lang_hover(sig));
    }

    if let Some(param) = query::param_at(state, line, col) {
        let sig = if param.is_type_param {
            format!("type {}", param.name)
        } else if param.type_str.is_empty() {
            param.name.clone()
        } else {
            format!("{}: {}", param.name, param.type_str)
        };
        return Some(make_lang_hover(sig));
    }

    None
}

pub(crate) fn make_lang_hover(value: String) -> Hover {
    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: format!("```varn\n{value}\n```"),
        }),
        range: None,
    }
}
