mod format;
mod imports;
mod intrinsics;
mod members;
mod symbols;

pub use intrinsics::{decorator_hover, intrinsic_or_keyword_hover};
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

        // Handle 'this' keyword via Checker scope resolution
        if tok.kind == TokenKind::This {
            if let Some((_, ty)) = state.db.resolve_at("this", tok.offset) {
                let ty_str = ty.to_string();
                if !ty_str.is_empty() && ty_str != "unknown" && ty_str != "dynamic" {
                    return Some(make_lang_hover(format!("this: {}", ty_str)));
                }
            }
            return Some(make_lang_hover("this".to_owned()));
        }

        // Handle decorators e.g. @inline, @deprecated
        let prev_is_at = idx
            .checked_sub(1)
            .and_then(|j| state.tokens.get(j))
            .map(|t| t.kind == TokenKind::At)
            .unwrap_or(false);
        if prev_is_at {
            if let Some(h) = decorator_hover(&tok.lexeme) {
                return Some(h);
            }
        }

        // Direct semantic MemberResolution from Checker
        if let Some(mem_res) = state.db.member_resolutions.get(&tok.offset) {
            let parent_str = mem_res.receiver_ty.to_string();
            let sig = match mem_res.member_kind {
                varn_checker::ResolvedMemberKind::EnumMember => {
                    format!("(enum member) {}.{}", parent_str, mem_res.member_name)
                }
                varn_checker::ResolvedMemberKind::Method => {
                    if let varn_core::TypeKind::Fn(ft) = &mem_res.member_ty.0 {
                        let params = format_fn_params(&ft.params);
                        format!(
                            "(method) {}.{}({}): {}",
                            parent_str, mem_res.member_name, params, ft.return_type
                        )
                    } else {
                        format!("(method) {}.{}: {}", parent_str, mem_res.member_name, mem_res.member_ty)
                    }
                }
                varn_checker::ResolvedMemberKind::StaticMethod => {
                    if let varn_core::TypeKind::Fn(ft) = &mem_res.member_ty.0 {
                        let params = format_fn_params(&ft.params);
                        format!(
                            "(static method) {}.{}({}): {}",
                            parent_str, mem_res.member_name, params, ft.return_type
                        )
                    } else {
                        format!(
                            "(static method) {}.{}: {}",
                            parent_str, mem_res.member_name, mem_res.member_ty
                        )
                    }
                }
                varn_checker::ResolvedMemberKind::StaticProperty => {
                    format!(
                        "(static property) {}.{}: {}",
                        parent_str, mem_res.member_name, mem_res.member_ty
                    )
                }
                varn_checker::ResolvedMemberKind::ExtensionMethod => {
                    if let varn_core::TypeKind::Fn(ft) = &mem_res.member_ty.0 {
                        let params = format_fn_params(&ft.params);
                        format!(
                            "(extension method) {}.{}({}): {}",
                            parent_str, mem_res.member_name, params, ft.return_type
                        )
                    } else {
                        format!(
                            "(extension method) {}.{}: {}",
                            parent_str, mem_res.member_name, mem_res.member_ty
                        )
                    }
                }
                varn_checker::ResolvedMemberKind::ExtensionProperty => {
                    format!(
                        "(extension property) {}.{}: {}",
                        parent_str, mem_res.member_name, mem_res.member_ty
                    )
                }
                varn_checker::ResolvedMemberKind::Getter => {
                    format!("(getter) {}.{}: {}", parent_str, mem_res.member_name, mem_res.member_ty)
                }
                varn_checker::ResolvedMemberKind::Setter => {
                    format!("(setter) {}.{}: {}", parent_str, mem_res.member_name, mem_res.member_ty)
                }
                varn_checker::ResolvedMemberKind::Property => {
                    format!("(property) {}.{}: {}", parent_str, mem_res.member_name, mem_res.member_ty)
                }
            };
            return Some(make_lang_hover(sig));
        }
    }

    if let Some(res) = query::resolve_chain(state, line, col) {
        match res {
            ChainResult::Symbol(sym) => {
                if sym.is_from_stdlib {
                    if let Some((_, tok)) = tok_any {
                        if let Some(h) = intrinsic_or_keyword_hover(tok) {
                            return Some(h);
                        }
                    }
                }
                return Some(symbol_hover(sym));
            }
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
        if sym.is_from_stdlib {
            if let Some((_, tok)) = tok_any {
                if let Some(h) = intrinsic_or_keyword_hover(tok) {
                    return Some(h);
                }
            }
        }
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

    // Fallback: Check for primitive types, literals, and language intrinsics
    if let Some((_, tok)) = tok_any {
        if let Some(h) = intrinsic_or_keyword_hover(tok) {
            return Some(h);
        }
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

fn format_fn_params(params: &[varn_checker::types::FunctionParam]) -> String {
    params
        .iter()
        .map(|p| {
            if let Some(name) = &p.name {
                format!("{}: {}", name, p.ty)
            } else {
                format!("{}", p.ty)
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

