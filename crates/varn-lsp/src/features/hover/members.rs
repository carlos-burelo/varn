use crate::document::{MemberKind, MemberRecord};

use super::format::format_inner_member;

pub fn format_member_sig(parent_name: &str, member: &MemberRecord) -> String {
    if member.type_str.contains("Enum") {
        return format_enum_member(parent_name, &member.name, &member.init_value);
    }
    let is_type_like = matches!(
        member.kind,
        MemberKind::Class
            | MemberKind::Interface
            | MemberKind::Namespace
            | MemberKind::Enum
            | MemberKind::EnumMember
            | MemberKind::Struct
    );
    let static_kw = if member.is_static && !is_type_like {
        "static "
    } else {
        ""
    };
    let prefix = if parent_name.is_empty() || parent_name == member.name {
        String::new()
    } else {
        format!("{}.", parent_name)
    };

    match member.kind {
        MemberKind::Property | MemberKind::Variable | MemberKind::Getter => {
            let kw = if member.kind == MemberKind::Variable {
                "var"
            } else {
                "property"
            };
            format!(
                "({}) {}{}{}: {}",
                kw, static_kw, prefix, member.name, member.type_str
            )
        }
        MemberKind::Setter => {
            format!(
                "(property) {}{}{}({})",
                static_kw, prefix, member.name, member.params_str
            )
        }
        MemberKind::Constructor => {
            format!("(constructor) {}({})", parent_name, member.params_str)
        }
        MemberKind::Method | MemberKind::Function => {
            let kw = if member.kind == MemberKind::Function {
                "function"
            } else {
                "method"
            };
            if member.is_arrow {
                format!(
                    "(property) {}{}{}: {}",
                    static_kw, prefix, member.name, member.type_str
                )
            } else {
                format!(
                    "({}) {}{}{}({}): {}",
                    kw, static_kw, prefix, member.name, member.params_str, member.type_str
                )
            }
        }
        MemberKind::Class => format_nested_class(parent_name, member),
        MemberKind::Interface => format_nested_interface(parent_name, member),
        MemberKind::Namespace => format_nested_namespace(parent_name, member),
        MemberKind::Enum => {
            format!("(enum) {}{}.{}", static_kw, parent_name, member.name)
        }
        MemberKind::EnumMember => format_enum_member(parent_name, &member.name, &member.init_value),
        MemberKind::Struct => {
            format!("(struct) {}{}.{}", static_kw, parent_name, member.name)
        }
    }
}

pub fn format_enum_member(enum_name: &str, member_name: &str, init_value: &str) -> String {
    if init_value.is_empty() {
        format!("(enum member) {enum_name}.{member_name}")
    } else {
        format!("(enum member) {enum_name}.{member_name} = {init_value}")
    }
}

fn format_nested_class(parent_name: &str, m: &MemberRecord) -> String {
    use super::format::format_type_params_str;
    let tp = format_type_params_str(&m.ty);
    if m.members.is_empty() {
        return format!("(class) {}.{}{}", parent_name, m.name, tp);
    }
    let mut lines = vec![format!("(class) {}.{}{} {{", parent_name, m.name, tp)];
    for inner in &m.members {
        lines.push(format_inner_member(inner));
    }
    lines.push("}".to_owned());
    lines.join("\n")
}

fn format_nested_interface(parent_name: &str, m: &MemberRecord) -> String {
    use super::format::format_type_params_str;
    let tp = format_type_params_str(&m.ty);
    if m.members.is_empty() {
        return format!("(interface) {}.{}{}", parent_name, m.name, tp);
    }
    let mut lines = vec![format!("(interface) {}.{}{} {{", parent_name, m.name, tp)];
    for inner in &m.members {
        lines.push(format_inner_member(inner));
    }
    lines.push("}".to_owned());
    lines.join("\n")
}

fn format_nested_namespace(parent_name: &str, m: &MemberRecord) -> String {
    if m.members.is_empty() {
        return format!("(namespace) {}.{}", parent_name, m.name);
    }
    let mut lines = vec![format!("(namespace) {}.{} {{", parent_name, m.name)];
    for inner in &m.members {
        lines.push(format_inner_member(inner));
    }
    lines.push("}".to_owned());
    lines.join("\n")
}
