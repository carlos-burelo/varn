mod format;
mod imports;
mod intrinsics;
mod members;
mod symbols;

pub use intrinsics::{decorator_hover, intrinsic_or_keyword_hover};
pub use members::{format_enum_member, format_member_sig};
pub use symbols::symbol_hover;

use tower_lsp::lsp_types::{Hover, HoverContents, MarkupContent, MarkupKind};
use varn_core::TokenKind;

use crate::document::{ChainResult, DocumentState};
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
                if !state.db.is_dynamic(&ty) {
                    return Some(make_lang_hover(format!("this: {}", state.ty_text(&ty))));
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
            let sig = member_resolution_sig(state, mem_res);
            return Some(make_lang_hover(sig));
        }
    }

    if let Some(res) = query::resolve_chain(state, line, col) {
        match res {
            ChainResult::Symbol(sym) => {
                if sym.is_from_stdlib() {
                    if let Some((_, tok)) = tok_any {
                        if let Some(h) = intrinsic_or_keyword_hover(tok) {
                            return Some(h);
                        }
                    }
                }
                return Some(symbol_hover(state, sym));
            }
            ChainResult::Member {
                member,
                parent_name,
            } => {
                let sig = if member.kind == varn_checker::ResolvedMemberKind::EnumMember {
                    format_enum_member(&parent_name, &member.name, "")
                } else {
                    format_member_sig(state, &parent_name, &member)
                };
                return Some(make_lang_hover(sig));
            }
        }
    }

    if let Some(sym) = query::symbol_at(state, line, col) {
        if sym.is_from_stdlib() {
            if let Some((_, tok)) = tok_any {
                if let Some(h) = intrinsic_or_keyword_hover(tok) {
                    return Some(h);
                }
            }
        }
        return Some(symbol_hover(state, sym));
    }

    if let Some((parent_name, member)) = query::member_at(state, line, col) {
        // The member's own kind decides; the parent's kind was only ever a
        // proxy for it, and the checker states it outright.
        let sig = if member.kind == varn_checker::ResolvedMemberKind::EnumMember {
            format_enum_member(&parent_name, &member.name, "")
        } else {
            format_member_sig(state, &parent_name, &member)
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

/// The hover for a member access the checker resolved.
fn member_resolution_sig(state: &DocumentState, res: &varn_checker::MemberResolution) -> String {
    use varn_checker::ResolvedMemberKind as R;
    let parent = state.ty_text(&res.receiver_ty);
    let name = &res.member_name;
    let ty = state.ty_text(&res.member_ty);
    let callable = |label: &str| match state.db.fn_shape(&res.member_ty) {
        Some(ft) => format!(
            "({label}) {parent}.{name}({}): {}",
            format_fn_params(state, &ft.params),
            state.db.id_text(ft.return_type)
        ),
        None => format!("({label}) {parent}.{name}: {ty}"),
    };
    let typed = |label: &str| format!("({label}) {parent}.{name}: {ty}");
    match res.member_kind {
        R::EnumMember => format!("(enum member) {parent}.{name}"),
        R::Method => callable("method"),
        R::StaticMethod => callable("static method"),
        R::ExtensionMethod => callable("extension method"),
        R::StaticProperty => typed("static property"),
        R::ExtensionProperty => typed("extension property"),
        R::Getter => typed("getter"),
        R::Setter => typed("setter"),
        R::Property => typed("property"),
        R::Constructor => format!(
            "(constructor) {parent}({})",
            format_member_params(state, &res.member_ty)
        ),
        // A nested type reads as the declaration it is — `class A.B`, not
        // `(property) A.B: B`.
        R::NestedType(k) => format!("{} {parent}.{name}", k.label()),
    }
}

fn format_fn_params(
    state: &DocumentState,
    params: &[varn_checker::types::FunctionParam],
) -> String {
    params
        .iter()
        .map(|p| {
            let ty = state.db.id_text(p.ty);
            match &p.name {
                Some(name) => format!("{name}: {ty}"),
                None => ty,
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// The parameter list of a member whose type is a function, else empty.
fn format_member_params(state: &DocumentState, ty: &varn_checker::Type) -> String {
    state
        .db
        .fn_shape(ty)
        .map(|ft| format_fn_params(state, &ft.params))
        .unwrap_or_default()
}
