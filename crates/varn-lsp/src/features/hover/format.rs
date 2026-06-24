use crate::document::{MemberKind, MemberRecord, SymbolRecord};
use crate::util::kinds::{member_kind_label, symbol_kind_label};
use varn_checker::SymbolKind;
use varn_core::TypeTag;

pub fn format_type_params(type_params: &[String]) -> String {
    if type_params.is_empty() {
        String::new()
    } else {
        format!("<{}>", type_params.join(", "))
    }
}

pub fn format_type_params_str(ty: &varn_checker::Type) -> String {
    match &ty.0 {
        varn_core::TypeKind::Generic(_, args, _) => {
            let names: Vec<String> = args.iter().map(|a| a.to_string()).collect();
            format!("<{}>", names.join(", "))
        }
        _ => String::new(),
    }
}

pub fn format_signature(sym: &SymbolRecord) -> String {
    match sym.kind {
        SymbolKind::Function | SymbolKind::Method => format_fn(sym),
        SymbolKind::Class => format_class(sym),
        SymbolKind::Struct => format!("struct {}", sym.name),
        SymbolKind::Interface => format_interface(sym),
        SymbolKind::TypeAlias => format!("type {} = {}", sym.name, sym.type_str),
        SymbolKind::Enum => format!("enum {}", sym.name),
        SymbolKind::Namespace => format_namespace(sym),
        SymbolKind::Const => format_binding("const", sym),
        SymbolKind::Let => format_binding("let", sym),
        SymbolKind::Var => format_binding("var", sym),
        SymbolKind::Parameter => format_binding("(param)", sym),
        SymbolKind::Property => format_binding("prop", sym),
        SymbolKind::Extension => {
            if sym.type_str.is_empty() {
                format!("extension {}", sym.name)
            } else {
                format!("extension {} on {}", sym.name, sym.type_str)
            }
        }
        SymbolKind::TypeParameter => format!("type {}", sym.name),
        SymbolKind::EnumMember => format_binding("(enum member)", sym),
    }
}

fn format_fn(sym: &SymbolRecord) -> String {
    let async_prefix = if sym.is_async { "async " } else { "" };
    let kw = symbol_kind_label(sym.kind);
    let tp = format_type_params(&sym.type_params);
    let gen_star = if sym.is_generator { "*" } else { "" };
    if sym.is_arrow {
        return format!(
            "{}{}{} {}{}: {}",
            async_prefix, kw, gen_star, sym.name, tp, sym.type_str
        );
    }
    format!(
        "{}{}{} {}{}({}): {}",
        async_prefix, kw, gen_star, sym.name, tp, sym.params_str, sym.type_str
    )
}

fn is_primitive_class_name(name: &str) -> bool {
    TypeTag::from_str(name)
        .map(|t| t.is_primitive())
        .unwrap_or(false)
}

fn format_class(sym: &SymbolRecord) -> String {
    let tp = format_type_params(&sym.type_params);
    if sym.members.is_empty() || is_primitive_class_name(&sym.name) {
        return format!("class {}{}", sym.name, tp);
    }
    let mut lines = vec![format!("class {}{} {{", sym.name, tp)];
    for m in &sym.members {
        lines.push(format_inner_member(m));
    }
    lines.push("}".to_owned());
    lines.join("\n")
}

fn format_interface(sym: &SymbolRecord) -> String {
    let tp = format_type_params(&sym.type_params);
    if sym.members.is_empty() {
        return format!("interface {}{}", sym.name, tp);
    }
    let mut lines = vec![format!("interface {}{} {{", sym.name, tp)];
    for m in &sym.members {
        lines.push(format_inner_member(m));
    }
    lines.push("}".to_owned());
    lines.join("\n")
}

fn format_namespace(sym: &SymbolRecord) -> String {
    if sym.members.is_empty() {
        return format!("namespace {}", sym.name);
    }
    let mut lines = vec![format!("namespace {} {{", sym.name)];
    for m in &sym.members {
        lines.push(format_inner_member(m));
    }
    lines.push("}".to_owned());
    lines.join("\n")
}

fn format_binding(keyword: &str, sym: &SymbolRecord) -> String {
    if sym.type_str.is_empty() {
        format!("{} {}", keyword, sym.name)
    } else {
        format!("{} {}: {}", keyword, sym.name, sym.type_str)
    }
}

pub fn format_inner_member(m: &MemberRecord) -> String {
    let indent = "  ";
    let static_prefix = if m.is_static { "static " } else { "" };
    match m.kind {
        MemberKind::Constructor => format!("{}constructor({})", indent, m.params_str),
        MemberKind::Method | MemberKind::Function => format!(
            "{}{}{}({}): {}",
            indent, static_prefix, m.name, m.params_str, m.type_str
        ),
        MemberKind::Property | MemberKind::Variable => {
            format!("{}{}{}: {}", indent, static_prefix, m.name, m.type_str)
        }
        MemberKind::Getter => format!(
            "{}{}get {}(): {}",
            indent, static_prefix, m.name, m.type_str
        ),
        MemberKind::Setter
        | MemberKind::Class
        | MemberKind::Interface
        | MemberKind::Namespace
        | MemberKind::Enum
        | MemberKind::EnumMember
        | MemberKind::Struct => {
            format!(
                "{}{}{} {}({})",
                indent,
                static_prefix,
                member_kind_label(m.kind),
                m.name,
                m.params_str
            )
        }
    }
}
