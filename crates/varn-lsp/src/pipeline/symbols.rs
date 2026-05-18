use std::collections::HashMap;

use super::format::extract_enum_init_value;
use crate::document::{MemberKind, MemberRecord, SymbolRecord, TokenRecord};
use varn_checker::types::FunctionType;
use varn_checker::{BindResult, ClassMemberInfo, ClassMemberKind, SymbolKind};
use varn_core::TypeKind;

pub fn map_members(ms: &[ClassMemberInfo], tokens: &[TokenRecord]) -> Vec<MemberRecord> {
    map_members_inner(ms, tokens, false)
}

pub fn map_enum_members(ms: &[ClassMemberInfo], tokens: &[TokenRecord]) -> Vec<MemberRecord> {
    map_members_inner(ms, tokens, true)
}

fn map_members_inner(
    ms: &[ClassMemberInfo],
    tokens: &[TokenRecord],
    is_enum: bool,
) -> Vec<MemberRecord> {
    ms.iter()
        .map(|m| {
            let type_str = if let TypeKind::Fn(ft) = &m.ty.0 {
                if !ft.is_arrow {
                    ft.return_type.to_string()
                } else {
                    m.ty.to_string()
                }
            } else if matches!(
                m.kind,
                ClassMemberKind::Class
                    | ClassMemberKind::Namespace
                    | ClassMemberKind::Interface
                    | ClassMemberKind::Enum
                    | ClassMemberKind::Struct
            ) {
                m.name.to_string()
            } else {
                m.ty.to_string()
            };

            MemberRecord {
                name: m.name.to_string(),
                type_str,
                params_str: m.params_str(),
                is_static: m.is_static,
                is_optional: m.is_optional,
                kind: if is_enum && m.kind == ClassMemberKind::Property {
                    MemberKind::EnumMember
                } else {
                    class_member_kind(&m.kind)
                },
                is_arrow: if let TypeKind::Fn(ft) = &m.ty.0 {
                    ft.is_arrow
                } else {
                    false
                },
                is_async: m.is_async,
                is_generator: m.is_generator,
                line: m.line,
                col: m.col,
                init_value: if m.kind == ClassMemberKind::Property
                    && m.return_type_str().contains("Enum")
                {
                    extract_enum_init_value(tokens, m.line)
                } else {
                    String::new()
                },
                ty: m.ty.clone(),
                symbol_id: m.symbol_id,
                members: map_members(&m.members, tokens),
            }
        })
        .collect()
}

pub fn class_member_kind(k: &ClassMemberKind) -> MemberKind {
    match k {
        ClassMemberKind::Constructor => MemberKind::Constructor,
        ClassMemberKind::Method => MemberKind::Method,
        ClassMemberKind::Function => MemberKind::Function,
        ClassMemberKind::Property => MemberKind::Property,
        ClassMemberKind::Variable => MemberKind::Variable,
        ClassMemberKind::Getter => MemberKind::Getter,
        ClassMemberKind::Setter => MemberKind::Setter,
        ClassMemberKind::Class => MemberKind::Class,
        ClassMemberKind::Interface => MemberKind::Interface,
        ClassMemberKind::Namespace => MemberKind::Namespace,
        ClassMemberKind::Enum => MemberKind::Enum,
        ClassMemberKind::Struct => MemberKind::Struct,
    }
}

pub fn inject_stdlib_symbols(
    symbols: &mut Vec<SymbolRecord>,
    symbol_map: &mut HashMap<String, SymbolKind>,
    bind: &BindResult,
    tokens: &[TokenRecord],
) {
    for sym in bind.global_symbols() {
        if sym.origin_module.is_none() || symbol_map.contains_key(sym.name.as_ref()) {
            continue;
        }

        let inferred_ty = sym.ty.clone().unwrap_or(varn_checker::types::Type::Dynamic);
        let type_str = if let TypeKind::Fn(FunctionType {
            return_type,
            is_arrow: false,
            ..
        }) = &inferred_ty.0
        {
            return_type.to_string()
        } else {
            inferred_ty.to_string()
        };

        let members = if sym.kind == SymbolKind::Namespace {
            bind.type_members
                .namespaces
                .get(&sym.name)
                .map(|ms| map_members(ms, tokens))
                .unwrap_or_default()
        } else if sym.kind == SymbolKind::Class || sym.kind == SymbolKind::Interface {
            bind.type_members
                .classes
                .get(&sym.name)
                .map(|e| map_members(&e.members, tokens))
                .or_else(|| {
                    bind.type_members
                        .interfaces
                        .get(&sym.name)
                        .map(|ms| map_members(ms, tokens))
                })
                .unwrap_or_default()
        } else if sym.kind == SymbolKind::Enum {
            bind.type_members
                .enums
                .get(&sym.name)
                .map(|ms| map_enum_members(ms, tokens))
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        symbols.push(SymbolRecord {
            name: sym.name.to_string(),
            kind: sym.kind,
            type_str,
            params_str: super::format::format_type_params(&inferred_ty),
            line: sym.line.saturating_sub(1),
            col: sym.col,
            end_line: if sym.full_range.end.line > 0 {
                sym.full_range.end.line.saturating_sub(1)
            } else {
                sym.line.saturating_sub(1)
            },
            end_col: if sym.full_range.end.line > 0 {
                sym.full_range.end.column
            } else {
                sym.col + sym.name.len() as u32
            },
            has_explicit_type: sym.has_explicit_type,
            is_async: sym.is_async,
            is_generator: sym.is_generator,
            is_arrow: matches!(
                &inferred_ty.0,
                TypeKind::Fn(FunctionType { is_arrow: true, .. })
            ),
            doc: sym.doc.as_deref().map(|s| s.to_owned()),
            members,
            type_params: sym.type_params.iter().map(|s| s.to_string()).collect(),
            ty: inferred_ty,
            symbol_id: None,
            full_range: sym.full_range,
            is_from_stdlib: true,
            origin: sym.origin_module.as_deref().map(|s| s.to_owned()),
        });
        symbol_map.insert(sym.name.to_string(), sym.kind);
    }
}
