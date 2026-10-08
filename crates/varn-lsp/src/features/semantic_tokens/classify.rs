use varn_checker::{SymbolKind, Type};
use varn_core::{is_lang_type_name, TokenKind, TypeKind};

use super::{
    TT_CLASS, TT_ENUM_MEMBER, TT_FUNCTION, TT_INTERFACE, TT_KEYWORD, TT_NAMESPACE, TT_NUMBER,
    TT_PARAMETER, TT_PROPERTY, TT_STRING, TT_TYPE, TT_TYPE_PARAMETER, TT_VARIABLE,
};
use crate::document::{DocumentState, TokenRecord};

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
        EOF | Dynamic | Identifier | RegularExpression | LParen | RParen | LBrace | RBrace
        | LBracket | RBracket | LAngle | RAngle | Semicolon | Comma | Dot | DotDot | DotDotDot
        | DotDotEq | Colon | ColonColon | Question | QuestionDot | QuestionLBracket
        | QuestionQuestion | QuestionQuestionEq | Plus | PlusPlus | PlusEq | Minus | MinusMinus
        | MinusEq | Star | StarStar | StarEq | StarStarEq | Slash | SlashEq | Percent
        | PercentEq | Amp | AmpAmp | AmpEq | AmpAmpEq | Pipe | PipePipe | PipeEq | PipePipeEq
        | Caret | CaretEq | Tilde | LtLt | LtLtEq | GtGt | GtGtEq | GtGtGt | GtGtGtEq | Eq
        | EqEq | EqEqEq | Bang | BangEq | BangEqEq | Lt | LtEq | Gt | GtEq | Let | Const | Var
        | Function | Class | Struct | Interface | Type | Enum | Namespace | Module | Extension
        | On | If | Else | Switch | Case | Default | While | For | Do | Break | Continue
        | Return | Throw | Try | Catch | Finally | Using | With | Import | Export | From | As
        | Async | Await | Yield | New | Super | Delete | Typeof | Instanceof | In | Of | Is
        | Public | Private | Protected | Static | Abstract | Override | Readonly | Declare
        | Native | Extends | Implements | Get | Set | Constructor | Destructor | Match | At
        | Hash | Backslash | Dollar | Backtick | Newline | Whitespace | DocComment
        | Placeholder | Spawn | Parallel | Start | RawStr => {}
    }

    if tok.kind.is_keyword() && !prev_is_dot && !getset_as_ident {
        return Some(TT_KEYWORD);
    }

    if prev_is_dot && prev2_is_enum {
        return Some(TT_ENUM_MEMBER);
    }

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
            varn_checker::ResolvedMemberKind::Constructor => TT_FUNCTION,

            varn_checker::ResolvedMemberKind::NestedType(k) => match k {
                varn_checker::NestedTypeKind::Interface => TT_INTERFACE,
                varn_checker::NestedTypeKind::Namespace => TT_NAMESPACE,
                varn_checker::NestedTypeKind::Enum => TT_TYPE,
                varn_checker::NestedTypeKind::Class | varn_checker::NestedTypeKind::Struct => {
                    TT_CLASS
                }
            },
        });
    }

    if let Some(info) = state.db.expr_types.get(&tok.offset) {
        if let Some(sid) = info.symbol_id.filter(|s| *s < state.db.bind.arena.len()) {
            let sym = state.db.bind.arena.get(sid);

            if state.name(sym.name) == state.lexeme(tok) {
                return Some(tt_from_symbol(state, sym.kind, &info.ty, prev_is_dot));
            }
            if prev_is_dot {
                return Some(member_tt(state, &info.ty));
            }
        } else if prev_is_dot {
            return Some(member_tt(state, &info.ty));
        }
    }

    if prev_is_dot {
        return Some(if next_is_lparen {
            TT_FUNCTION
        } else {
            TT_PROPERTY
        });
    }

    if next_is_colon {
        return Some(TT_PROPERTY);
    }

    if let Some((sid, ty)) = state.db.resolve_at(state.lexeme(tok), tok.offset) {
        if sid < state.db.bind.arena.len() {
            return Some(tt_from_symbol(
                state,
                state.db.bind.arena.get(sid).kind,
                &ty,
                prev_is_dot,
            ));
        }
    }

    if is_lang_type_name(state.lexeme(tok)) {
        return Some(TT_TYPE);
    }

    if state.type_param_names.contains(state.lexeme(tok)) {
        return Some(TT_TYPE_PARAMETER);
    }
    if tok.kind.is_keyword() {
        return Some(TT_KEYWORD);
    }
    None
}

fn member_tt(state: &DocumentState, ty: &Type) -> u32 {
    if matches!(state.db.ty_kind(ty), TypeKind::Fn(_)) {
        TT_FUNCTION
    } else {
        TT_PROPERTY
    }
}

fn tt_from_symbol(state: &DocumentState, kind: SymbolKind, ty: &Type, prev_is_dot: bool) -> u32 {
    let is_fn = matches!(state.db.ty_kind(ty), TypeKind::Fn(_));
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
    match state.db.ty_kind(ty) {
        TypeKind::Named(n, _) | TypeKind::Generic(n, _, _) => {
            matches!(state.symbol_map.get(state.name(n)), Some(SymbolKind::Enum))
        }
        TypeKind::Primitive(_)
        | TypeKind::Builtin(_)
        | TypeKind::Literal(_)
        | TypeKind::This
        | TypeKind::Array(_)
        | TypeKind::Union(_)
        | TypeKind::Intersection(_)
        | TypeKind::Tuple(_)
        | TypeKind::TemplateLiteral(_)
        | TypeKind::Fn(_)
        | TypeKind::Object(_)
        | TypeKind::Typeof(_)
        | TypeKind::KeyOf(_)
        | TypeKind::IndexedAccess { .. }
        | TypeKind::Mapped { .. }
        | TypeKind::Conditional { .. }
        | TypeKind::Infer(_)
        | TypeKind::EnumVariant { .. }
        | TypeKind::TypePredicate { .. } => false,
    }
}
