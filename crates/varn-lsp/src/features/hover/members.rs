use crate::document::DocumentState;
use varn_checker::{NestedTypeKind, ResolvedMemberKind, ResolvedMemberSummary};

use super::format::{format_summary_member, format_type_params_str};

/// What `dynamic` prints as: a receiver that names no type.
const DYNAMIC: &str = varn_core::LangPrimitive::Dynamic.name();

pub fn format_member_sig(
    state: &DocumentState,
    parent_name: &str,
    member: &ResolvedMemberSummary,
) -> String {
    let clean_parent = parent_name.trim().trim_end_matches(['.', ')', '(']);
    let has_parent =
        !clean_parent.is_empty() && clean_parent != DYNAMIC && clean_parent != member.name.as_ref();
    let name = &member.name;
    let owner = if has_parent {
        format!("{clean_parent}.")
    } else {
        String::new()
    };
    let is_static = member.is_static && has_parent;

    match member.kind {
        ResolvedMemberKind::Property
        | ResolvedMemberKind::StaticProperty
        | ResolvedMemberKind::ExtensionProperty
        | ResolvedMemberKind::Getter => {
            let label = if is_static {
                "static property"
            } else {
                "property"
            };
            format!("({label}) {owner}{name}: {}", state.ty_text(&member.ty))
        }
        ResolvedMemberKind::Setter => {
            let label = if is_static { "static setter" } else { "setter" };
            format!("({label}) {owner}{name}({})", member_params(state, member))
        }
        ResolvedMemberKind::Constructor => {
            let class = if has_parent {
                format!(" {clean_parent}")
            } else {
                String::new()
            };
            format!("constructor{class}({})", member_params(state, member))
        }
        ResolvedMemberKind::Method
        | ResolvedMemberKind::StaticMethod
        | ResolvedMemberKind::ExtensionMethod => {
            let params = member_params(state, member);
            let ret = member_return(state, member);
            if is_static {
                format!("(static method) {owner}{name}({params}): {ret}")
            } else if has_parent {
                format!("(method) {owner}{name}({params}): {ret}")
            } else {
                format!("function {name}({params}): {ret}")
            }
        }
        ResolvedMemberKind::NestedType(k @ (NestedTypeKind::Class | NestedTypeKind::Interface)) => {
            let tp = format_type_params_str(state, &member.ty);
            format_nested(state, k.label(), &owner, member, &tp)
        }
        ResolvedMemberKind::NestedType(k @ NestedTypeKind::Namespace) => {
            format_nested(state, k.label(), &owner, member, "")
        }
        ResolvedMemberKind::NestedType(k @ (NestedTypeKind::Enum | NestedTypeKind::Struct)) => {
            format!("{} {owner}{name}", k.label())
        }
        ResolvedMemberKind::EnumMember => format_enum_member(clean_parent, name, ""),
    }
}

pub fn format_enum_member(enum_name: &str, member_name: &str, init_value: &str) -> String {
    let clean_enum = enum_name.trim().trim_end_matches(['.', ')', '(']);
    let prefix = if !clean_enum.is_empty() && clean_enum != DYNAMIC {
        format!("{}.", clean_enum)
    } else {
        String::new()
    };

    if init_value.is_empty() {
        format!("(enum member) {prefix}{member_name}")
    } else {
        format!("(enum member) {prefix}{member_name} = {init_value}")
    }
}

/// A nested class, interface or namespace, with its own body — asked of the
/// checker by the name it declares.
fn format_nested(
    state: &DocumentState,
    keyword: &str,
    owner: &str,
    m: &ResolvedMemberSummary,
    type_params: &str,
) -> String {
    let head = format!("{keyword} {owner}{}{type_params}", m.name);
    let inner_members = state.members_of_type(&state.db.named_type(&m.name));
    if inner_members.is_empty() {
        return head;
    }
    let mut lines = vec![format!("{head} {{")];
    for inner in &inner_members {
        lines.push(format_summary_member(state, inner));
    }
    lines.push("}".to_owned());
    lines.join("\n")
}

/// A member's parameter list, when its type is a function.
fn member_params(state: &DocumentState, m: &ResolvedMemberSummary) -> String {
    let Some(ft) = state.db.fn_shape(&m.ty) else {
        return String::new();
    };
    ft.params
        .iter()
        .map(|p| {
            format!(
                "{}: {}",
                p.name.as_deref().unwrap_or("arg"),
                state.db.id_text(p.ty)
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// What calling a method member returns: its function type's return type.
fn member_return(state: &DocumentState, m: &ResolvedMemberSummary) -> String {
    match state.db.fn_shape(&m.ty) {
        Some(ft) => state.db.id_text(ft.return_type),
        None => state.ty_text(&m.ty),
    }
}
