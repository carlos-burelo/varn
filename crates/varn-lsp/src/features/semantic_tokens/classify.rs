use varn_checker::{SymbolKind, Type};
use varn_core::{IntrinsicType, TokenKind, TypeKind, TypeTag};

use super::{
    TT_CLASS, TT_ENUM_MEMBER, TT_FUNCTION, TT_INTERFACE, TT_KEYWORD, TT_NAMESPACE, TT_NUMBER,
    TT_PARAMETER, TT_PROPERTY, TT_STRING, TT_TYPE, TT_TYPE_PARAMETER, TT_VARIABLE,
};
use crate::document::{DocumentState, TokenRecord};

/// Resolve a token to its semantic-token type, driven by the checker.
///
/// Order of authority:
///   1. Fixed token kinds (literals, `this`, arrows) and hard keywords.
///   2. `expr_types[offset]` — the per-occurrence type+symbol the checker
///      recorded for this exact source position.
///   3. `resolve_at(name, offset)` — lexical scope resolution (params, type
///      parameters, locals, globals), honouring shadowing.
///   4. Structural syntax that is not a symbol (member name on a dynamic value,
///      object-literal key, builtin type name).
///
/// Steps 2–3 are the checker; the heuristics it replaced (token-scanned param
/// scopes, name-keyed symbol maps, object-key look-ahead) are gone.
pub fn resolve_token(
    state: &DocumentState,
    tok: &TokenRecord,
    prev_is_dot: bool,
    prev2_is_enum: bool,
    next_is_lparen: bool,
    next_is_colon: bool,
    getset_as_ident: bool,
) -> Option<u32> {
    use TokenKind::*;

    match tok.kind {
        True | False | Null => return Some(TT_NUMBER),
        This => return Some(TT_VARIABLE),
        Void => return Some(TT_TYPE),
        Arrow | FatArrow | PipeGt => return Some(TT_KEYWORD),
        IntegerLiteral | FloatLiteral | BinaryLiteral | OctalLiteral | HexLiteral
        | BigIntLiteral | DecimalLiteral => return Some(TT_NUMBER),
        Str | Char | Template | TemplateHead | TemplateMiddle | TemplateTail => {
            return Some(TT_STRING)
        }
        _ => {}
    }

    // Hard keyword: a keyword token that is neither a member name nor a
    // contextual identifier (`get`/`set` used as a name). Contextual keywords
    // fall through to resolution.
    if tok.kind.is_keyword() && !prev_is_dot && !getset_as_ident {
        return Some(TT_KEYWORD);
    }

    // `Enum.Variant` — the variant is an enum member regardless of how the
    // checker models it (nullary variant = Property of the enum type, payload
    // variant = a constructor function). The receiver being an enum *type* is
    // the discriminator (`Ok.code` where the receiver is a value is a field).
    if prev_is_dot && prev2_is_enum {
        return Some(TT_ENUM_MEMBER);
    }

    // (2) The checker recorded this exact occurrence.
    if let Some(mem_res) = state.db.member_resolutions.get(&tok.offset) {
        return Some(match mem_res.member_kind {
            varn_checker::ResolvedMemberKind::EnumMember => TT_ENUM_MEMBER,
            varn_checker::ResolvedMemberKind::Method
            | varn_checker::ResolvedMemberKind::StaticMethod
            | varn_checker::ResolvedMemberKind::ExtensionMethod => TT_FUNCTION,
            varn_checker::ResolvedMemberKind::Property
            | varn_checker::ResolvedMemberKind::StaticProperty
            | varn_checker::ResolvedMemberKind::ExtensionProperty
            | varn_checker::ResolvedMemberKind::Getter
            | varn_checker::ResolvedMemberKind::Setter => TT_PROPERTY,
        });
    }

    if let Some(info) = state.db.expr_types.get(&tok.offset) {
        if let Some(sid) = info.symbol_id.filter(|s| *s < state.db.arena.len()) {
            let sym = state.db.arena.get(sid);
            // The symbol_id is only authoritative when it names this very token.
            // For some members the checker records the member's *type* symbol
            // (e.g. `arr.length` → the `int` class), which must not paint the
            // member as a class.
            if sym.name.as_ref() == tok.lexeme.as_str() {
                return Some(tt_from_symbol(state, sym.kind, &info.ty, prev_is_dot));
            }
            if prev_is_dot {
                return Some(member_tt(&info.ty));
            }
        } else if prev_is_dot {
            // Recorded with a type but no symbol (structural / dynamic member).
            return Some(member_tt(&info.ty));
        }
    }

    // Member access whose name the checker did not record: a property/method on
    // a dynamic value. Resolved before name lookup so an unrelated global of the
    // same name cannot capture it.
    if prev_is_dot {
        return Some(if next_is_lparen {
            TT_FUNCTION
        } else {
            TT_PROPERTY
        });
    }

    // Object-literal key / field label (`name:` / `name?:`). Not a symbol;
    // resolved before name lookup so a same-named binding elsewhere cannot
    // capture it. Real parameter/field declarations were already resolved via
    // their `expr_types` entry above.
    if next_is_colon {
        return Some(TT_PROPERTY);
    }

    // (3) Lexical scope resolution: locals, globals.
    if let Some((sid, ty)) = state.db.resolve_at(&tok.lexeme, tok.offset) {
        if sid < state.db.arena.len() {
            return Some(tt_from_symbol(
                state,
                state.db.arena.get(sid).kind,
                &ty,
                prev_is_dot,
            ));
        }
    }

    if is_intrinsic_type_name(&tok.lexeme) {
        return Some(TT_TYPE);
    }
    // Type parameters: exposed by the checker as TypeParameter symbols, but
    // references inside type annotations are not recorded per-offset. The name
    // set is built from those symbols (checker-sourced), not a token scan.
    if state.type_param_names.contains(tok.lexeme.as_str()) {
        return Some(TT_TYPE_PARAMETER);
    }
    if tok.kind.is_keyword() {
        return Some(TT_KEYWORD);
    }
    None
}

fn member_tt(ty: &Type) -> u32 {
    if matches!(ty.0, TypeKind::Fn(_)) {
        TT_FUNCTION
    } else {
        TT_PROPERTY
    }
}

fn tt_from_symbol(state: &DocumentState, kind: SymbolKind, ty: &Type, prev_is_dot: bool) -> u32 {
    let is_fn = matches!(ty.0, TypeKind::Fn(_));
    match kind {
        SymbolKind::Function | SymbolKind::Method => TT_FUNCTION,
        SymbolKind::Class | SymbolKind::Struct | SymbolKind::Extension => TT_CLASS,
        SymbolKind::Interface => TT_INTERFACE,
        SymbolKind::Namespace => TT_NAMESPACE,
        SymbolKind::TypeAlias | SymbolKind::Enum => TT_TYPE,
        SymbolKind::EnumMember => TT_ENUM_MEMBER,
        SymbolKind::TypeParameter => TT_TYPE_PARAMETER,
        SymbolKind::Parameter => {
            if is_fn {
                TT_FUNCTION
            } else {
                TT_PARAMETER
            }
        }
        SymbolKind::Property => {
            // `Enum.Variant` value access: the binder models a nullary variant as
            // a Property whose type is the enum itself. Surface it as an enum
            // member when accessed through the enum type.
            if prev_is_dot && is_enum_type(state, ty) {
                TT_ENUM_MEMBER
            } else if is_fn {
                TT_FUNCTION
            } else {
                TT_PROPERTY
            }
        }
        SymbolKind::Const | SymbolKind::Let | SymbolKind::Var => {
            if is_fn {
                TT_FUNCTION
            } else {
                TT_VARIABLE
            }
        }
    }
}

fn is_enum_type(state: &DocumentState, ty: &Type) -> bool {
    match &ty.0 {
        TypeKind::Named(n, _) | TypeKind::Generic(n, _, _) => {
            matches!(state.symbol_map.get(n.as_ref()), Some(SymbolKind::Enum))
        }
        _ => false,
    }
}

fn is_intrinsic_type_name(name: &str) -> bool {
    name == IntrinsicType::Str.as_str()
        || name == IntrinsicType::Int.as_str()
        || name == IntrinsicType::Float.as_str()
        || name == IntrinsicType::Decimal.as_str()
        || name == IntrinsicType::BigInt.as_str()
        || name == IntrinsicType::Char.as_str()
        || name == IntrinsicType::Bool.as_str()
        || name == IntrinsicType::Symbol.as_str()
        || name == TypeTag::Object.name()
        || name == IntrinsicType::Void.as_str()
        || name == IntrinsicType::Never.as_str()
        || name == IntrinsicType::Dynamic.as_str()
        || name == IntrinsicType::Null.as_str()
        || name == IntrinsicType::Task.as_str()
        || name == IntrinsicType::Result.as_str()
        || name == IntrinsicType::Array.as_str()
}
