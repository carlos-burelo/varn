use crate::document::{MemberKind, MemberRecord};

use super::format::format_inner_member;

pub fn format_member_sig(parent_name: &str, member: &MemberRecord) -> String {
    if member.type_str.contains("Enum") || member.kind == MemberKind::EnumMember {
        return format_enum_member(parent_name, &member.name, &member.init_value);
    }

    let clean_parent = parent_name.trim().trim_end_matches(['.', ')', '(']);
    let has_parent = !clean_parent.is_empty() && clean_parent != "dynamic" && clean_parent != member.name;

    match member.kind {
        MemberKind::Property | MemberKind::Variable | MemberKind::Getter => {
            if member.is_static && has_parent {
                format!("(static property) {}.{}: {}", clean_parent, member.name, member.type_str)
            } else if has_parent {
                format!("(property) {}.{}: {}", clean_parent, member.name, member.type_str)
            } else {
                format!("(property) {}: {}", member.name, member.type_str)
            }
        }
        MemberKind::Setter => {
            if member.is_static && has_parent {
                format!("(static setter) {}.{}({})", clean_parent, member.name, member.params_str)
            } else if has_parent {
                format!("(setter) {}.{}({})", clean_parent, member.name, member.params_str)
            } else {
                format!("(setter) {}({})", member.name, member.params_str)
            }
        }
        MemberKind::Constructor => {
            if has_parent {
                format!("constructor {}({})", clean_parent, member.params_str)
            } else {
                format!("constructor({})", member.params_str)
            }
        }
        MemberKind::Method | MemberKind::Function => {
            if member.is_static && has_parent {
                format!(
                    "(static method) {}.{}({}): {}",
                    clean_parent, member.name, member.params_str, member.type_str
                )
            } else if has_parent {
                format!(
                    "(method) {}.{}({}): {}",
                    clean_parent, member.name, member.params_str, member.type_str
                )
            } else {
                format!(
                    "function {}({}): {}",
                    member.name, member.params_str, member.type_str
                )
            }
        }
        MemberKind::Class => format_nested_class(clean_parent, member),
        MemberKind::Interface => format_nested_interface(clean_parent, member),
        MemberKind::Namespace => format_nested_namespace(clean_parent, member),
        MemberKind::Enum => {
            if has_parent {
                format!("enum {}.{}", clean_parent, member.name)
            } else {
                format!("enum {}", member.name)
            }
        }
        MemberKind::EnumMember => format_enum_member(clean_parent, &member.name, &member.init_value),
        MemberKind::Struct => {
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

fn format_nested_class(parent_name: &str, m: &MemberRecord) -> String {
    use super::format::format_type_params_str;
    let tp = format_type_params_str(&m.ty);
    let prefix = if !parent_name.is_empty() && parent_name != "dynamic" {
        format!("{}.", parent_name)
    } else {
        String::new()
    };
    if m.members.is_empty() {
        return format!("class {}{}{}", prefix, m.name, tp);
    }
    let mut lines = vec![format!("class {}{}{} {{", prefix, m.name, tp)];
    for inner in &m.members {
        lines.push(format_inner_member(inner));
    }
    lines.push("}".to_owned());
    lines.join("\n")
}

fn format_nested_interface(parent_name: &str, m: &MemberRecord) -> String {
    use super::format::format_type_params_str;
    let tp = format_type_params_str(&m.ty);
    let prefix = if !parent_name.is_empty() && parent_name != "dynamic" {
        format!("{}.", parent_name)
    } else {
        String::new()
    };
    if m.members.is_empty() {
        return format!("interface {}{}{}", prefix, m.name, tp);
    }
    let mut lines = vec![format!("interface {}{}{} {{", prefix, m.name, tp)];
    for inner in &m.members {
        lines.push(format_inner_member(inner));
    }
    lines.push("}".to_owned());
    lines.join("\n")
}

fn format_nested_namespace(parent_name: &str, m: &MemberRecord) -> String {
    let prefix = if !parent_name.is_empty() && parent_name != "dynamic" {
        format!("{}.", parent_name)
    } else {
        String::new()
    };
    if m.members.is_empty() {
        return format!("namespace {}{}", prefix, m.name);
    }
    let mut lines = vec![format!("namespace {}{} {{", prefix, m.name)];
    for inner in &m.members {
        lines.push(format_inner_member(inner));
    }
    lines.push("}".to_owned());
    lines.join("\n")
}
