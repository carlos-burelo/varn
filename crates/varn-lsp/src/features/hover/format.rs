use crate::document::{DocumentState, SymbolView};
use varn_checker::SymbolKind;
use varn_checker::{ResolvedMemberKind, ResolvedMemberSummary};
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

pub fn format_signature(state: &DocumentState, sym: SymbolView<'_>) -> String {
    match sym.kind() {
        SymbolKind::Function | SymbolKind::Method => format_fn(sym),
        SymbolKind::Class => format_class(state, sym),
        SymbolKind::Struct => format!("struct {}", sym.name()),
        SymbolKind::Interface => format_interface(state, sym),
        SymbolKind::TypeAlias => format!("type {} = {}", sym.name(), sym.type_str()),
        SymbolKind::Enum => format!("enum {}", sym.name()),
        SymbolKind::Namespace => format_namespace(state, sym),
        SymbolKind::Const => format_binding("const", sym),
        SymbolKind::Let => format_binding("let", sym),
        SymbolKind::Var => format_binding("var", sym),
        SymbolKind::Parameter => format_param(sym),
        SymbolKind::Property => format_binding("let", sym),
        SymbolKind::Extension => {
            if sym.type_str().is_empty() {
                format!("extension {}", sym.name())
            } else {
                format!("extension {} on {}", sym.name(), sym.type_str())
            }
        }
        SymbolKind::TypeParameter => format!("type {}", sym.name()),
        SymbolKind::EnumMember => format_enum_member_sym(sym),
    }
}

fn format_fn(sym: SymbolView<'_>) -> String {
    let async_prefix = if sym.is_async() { "async " } else { "" };
    let kw = if sym.kind() == SymbolKind::Method {
        ""
    } else {
        "function "
    };
    let tp = format_type_params(&sym.type_params());
    let gen_star = if sym.is_generator() { "*" } else { "" };
    if sym.is_arrow() {
        return format!(
            "{}{}{} {}{}: {}",
            async_prefix,
            kw,
            gen_star,
            sym.name(),
            tp,
            sym.type_str()
        );
    }
    format!(
        "{}{}{}{}{}({}): {}",
        async_prefix,
        kw,
        gen_star,
        sym.name(),
        tp,
        sym.params_str(),
        sym.type_str()
    )
}

fn format_param(sym: SymbolView<'_>) -> String {
    if sym.type_str().is_empty() {
        sym.name().to_owned()
    } else {
        format!("{}: {}", sym.name(), sym.type_str())
    }
}

fn format_enum_member_sym(sym: SymbolView<'_>) -> String {
    if sym.type_str().is_empty() {
        sym.name().to_owned()
    } else {
        format!("{} = {}", sym.name(), sym.type_str())
    }
}

fn is_primitive_class_name(name: &str) -> bool {
    TypeTag::from_str(name)
        .map(|t| t.is_primitive())
        .unwrap_or(false)
}

fn format_class(state: &DocumentState, sym: SymbolView<'_>) -> String {
    let tp = format_type_params(&sym.type_params());
    let members = state.members_of(sym);
    if members.is_empty() || is_primitive_class_name(sym.name()) {
        return format!("class {}{}", sym.name(), tp);
    }
    let mut lines = vec![format!("class {}{} {{", sym.name(), tp)];
    for m in &members {
        lines.push(format_summary_member(m));
    }
    lines.push("}".to_owned());
    lines.join("\n")
}

fn format_interface(state: &DocumentState, sym: SymbolView<'_>) -> String {
    let tp = format_type_params(&sym.type_params());
    let members = state.members_of(sym);
    if members.is_empty() {
        return format!("interface {}{}", sym.name(), tp);
    }
    let mut lines = vec![format!("interface {}{} {{", sym.name(), tp)];
    for m in &members {
        lines.push(format_summary_member(m));
    }
    lines.push("}".to_owned());
    lines.join("\n")
}

fn format_namespace(state: &DocumentState, sym: SymbolView<'_>) -> String {
    let members = state.members_of(sym);
    if members.is_empty() {
        return format!("namespace {}", sym.name());
    }
    let mut lines = vec![format!("namespace {} {{", sym.name())];
    for m in &members {
        lines.push(format_summary_member(m));
    }
    lines.push("}".to_owned());
    lines.join("\n")
}

fn format_binding(keyword: &str, sym: SymbolView<'_>) -> String {
    if sym.type_str().is_empty() {
        format!("{} {}", keyword, sym.name())
    } else {
        format!("{} {}: {}", keyword, sym.name(), sym.type_str())
    }
}

/// Render one member of a class, interface or namespace body, from the
/// checker's summary of it.
///
/// The signature is derived from `m.ty` at render time rather than read out of
/// a pre-formatted `String`, so what the hover shows is what the checker
/// decided — generics substituted, extensions included.
pub fn format_summary_member(m: &ResolvedMemberSummary) -> String {
    let indent = "  ";
    let static_prefix = if m.is_static { "static " } else { "" };
    let optional = if m.optional { "?" } else { "" };

    match &m.ty.0 {
        varn_core::TypeKind::Fn(ft) if !matches!(m.kind, ResolvedMemberKind::Getter) => {
            let params = ft
                .params
                .iter()
                .map(|p| {
                    format!(
                        "{}: {}{}",
                        p.name.as_deref().unwrap_or("arg"),
                        p.ty,
                        if p.optional { "?" } else { "" }
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "{indent}{static_prefix}{}{optional}({params}): {}",
                m.name, ft.return_type
            )
        }
        _ => match m.kind {
            ResolvedMemberKind::Getter => {
                format!("{indent}{static_prefix}get {}(): {}", m.name, m.ty)
            }
            ResolvedMemberKind::Setter => {
                format!("{indent}{static_prefix}set {}({})", m.name, m.ty)
            }
            ResolvedMemberKind::EnumMember => format!("{indent}{}", m.name),
            _ => format!("{indent}{static_prefix}{}{optional}: {}", m.name, m.ty),
        },
    }
}
