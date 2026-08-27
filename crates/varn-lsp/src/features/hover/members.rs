use crate::document::DocumentState;
use varn_checker::{NestedTypeKind, ResolvedMemberKind, ResolvedMemberSummary};

use super::format::format_summary_member;

pub fn format_member_sig(
    state: &DocumentState,
    parent_name: &str,
    member: &ResolvedMemberSummary,
) -> String {
    if member.kind == ResolvedMemberKind::EnumMember {
        return format_enum_member(parent_name, &member.name, "");
    }

    let clean_parent = parent_name.trim().trim_end_matches(['.', ')', '(']);
    let has_parent = !clean_parent.is_empty()
        && clean_parent != varn_core::TypeTag::Dynamic.name()
        && clean_parent != member.name.as_ref();

    match member.kind {
        ResolvedMemberKind::Property | ResolvedMemberKind::StaticProperty | ResolvedMemberKind::ExtensionProperty | ResolvedMemberKind::Getter => {
            if member.is_static && has_parent {
                format!("(static property) {}.{}: {}", clean_parent, member.name, member.ty)
            } else if has_parent {
                format!("(property) {}.{}: {}", clean_parent, member.name, member.ty)
            } else {
                format!("(property) {}: {}", member.name, member.ty)
            }
        }
        ResolvedMemberKind::Setter => {
            if member.is_static && has_parent {
                format!("(static setter) {}.{}({})", clean_parent, member.name, member_params(member))
            } else if has_parent {
                format!("(setter) {}.{}({})", clean_parent, member.name, member_params(member))
            } else {
                format!("(setter) {}({})", member.name, member_params(member))
            }
        }
        ResolvedMemberKind::Constructor => {
            if has_parent {
                format!("constructor {}({})", clean_parent, member_params(member))
            } else {
                format!("constructor({})", member_params(member))
            }
        }
        ResolvedMemberKind::Method | ResolvedMemberKind::StaticMethod | ResolvedMemberKind::ExtensionMethod => {
            if member.is_static && has_parent {
                format!(
                    "(static method) {}.{}({}): {}",
                    clean_parent, member.name, member_params(member), member.ty
                )
            } else if has_parent {
                format!(
                    "(method) {}.{}({}): {}",
                    clean_parent, member.name, member_params(member), member.ty
                )
            } else {
                format!(
                    "function {}({}): {}",
                    member.name, member_params(member), member.ty
                )
            }
        }
        ResolvedMemberKind::NestedType(NestedTypeKind::Class) => format_nested_class(state, clean_parent, member),
        ResolvedMemberKind::NestedType(NestedTypeKind::Interface) => format_nested_interface(state, clean_parent, member),
        ResolvedMemberKind::NestedType(NestedTypeKind::Namespace) => format_nested_namespace(state, clean_parent, member),
        ResolvedMemberKind::NestedType(NestedTypeKind::Enum) => {
            if has_parent {
                format!("enum {}.{}", clean_parent, member.name)
            } else {
                format!("enum {}", member.name)
            }
        }
        ResolvedMemberKind::EnumMember => format_enum_member(clean_parent, &member.name, ""),
        ResolvedMemberKind::NestedType(NestedTypeKind::Struct) => {
            if has_parent {
                format!("struct {}.{}", clean_parent, member.name)
            } else {
                format!("struct {}", member.name)
            }
        }
    }
}

pub fn format_enum_member(enum_name: &str, member_name: &str, init_value: &str) -> String {
    let clean_enum = enum_name.trim().trim_end_matches(['.', ')', '(']);
    let prefix = if !clean_enum.is_empty() && clean_enum != "dynamic" {
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

fn format_nested_class(state: &DocumentState, parent_name: &str, m: &ResolvedMemberSummary) -> String {
    use super::format::format_type_params_str;
    let tp = format_type_params_str(&m.ty);
    let prefix = if !parent_name.is_empty() && parent_name != "dynamic" {
        format!("{}.", parent_name)
    } else {
        String::new()
    };
    // The nested type's own body, asked of the checker by the name it declares.
    let inner_members = state.members_of_type(&varn_checker::Type::named(m.name.clone()));
    if inner_members.is_empty() {
        return format!("class {}{}{}", prefix, m.name, tp);
    }
    let mut lines = vec![format!("class {}{}{} {{", prefix, m.name, tp)];
    for inner in &inner_members {
        lines.push(format_summary_member(inner));
    }
    lines.push("}".to_owned());
    lines.join("\n")
}

fn format_nested_interface(state: &DocumentState, parent_name: &str, m: &ResolvedMemberSummary) -> String {
    use super::format::format_type_params_str;
    let tp = format_type_params_str(&m.ty);
    let prefix = if !parent_name.is_empty() && parent_name != "dynamic" {
        format!("{}.", parent_name)
    } else {
        String::new()
    };
    // The nested type's own body, asked of the checker by the name it declares.
    let inner_members = state.members_of_type(&varn_checker::Type::named(m.name.clone()));
    if inner_members.is_empty() {
        return format!("interface {}{}{}", prefix, m.name, tp);
    }
    let mut lines = vec![format!("interface {}{}{} {{", prefix, m.name, tp)];
    for inner in &inner_members {
        lines.push(format_summary_member(inner));
    }
    lines.push("}".to_owned());
    lines.join("\n")
}

fn format_nested_namespace(state: &DocumentState, parent_name: &str, m: &ResolvedMemberSummary) -> String {
    let prefix = if !parent_name.is_empty() && parent_name != "dynamic" {
        format!("{}.", parent_name)
    } else {
        String::new()
    };
    let inner_members = state.members_of_type(&varn_checker::Type::named(m.name.clone()));
    if inner_members.is_empty() {
        return format!("namespace {}{}", prefix, m.name);
    }
    let mut lines = vec![format!("namespace {}{} {{", prefix, m.name)];
    for inner in &inner_members {
        lines.push(format_summary_member(inner));
    }
    lines.push("}".to_owned());
    lines.join("\n")
}

/// A member's parameter list, when its type is a function.
fn member_params(m: &ResolvedMemberSummary) -> String {
    match &m.ty.0 {
        varn_core::TypeKind::Fn(ft) => ft
            .params
            .iter()
            .map(|p| format!("{}: {}", p.name.as_deref().unwrap_or("arg"), p.ty))
            .collect::<Vec<_>>()
            .join(", "),
        _ => String::new(),
    }
}
