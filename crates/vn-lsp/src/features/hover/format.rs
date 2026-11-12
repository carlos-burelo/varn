use crate::document::{DocumentState, SymbolView};
use vn_sem::semantic_info::{ResolvedMemberKind, ResolvedMemberSummary};
use vn_sem::symbol::SymbolKind;

pub fn format_type_params(type_params: &[String]) -> String {
    if type_params.is_empty() {
        String::new()
    } else {
        format!("<{}>", type_params.join(", "))
    }
}

pub fn format_type_params_str(state: &DocumentState, ty: &vn_sem::types::Type) -> String {
    match state.db.ty_kind(ty) {
        vn_core::TypeKind::Generic(_, args, _) => {
            let names: Vec<String> = state
                .db
                .ty_list(args)
                .iter()
                .map(|a| state.ty_text(a))
                .collect();
            format!("<{}>", names.join(", "))
        }
        vn_core::TypeKind::Primitive(_)
        | vn_core::TypeKind::Literal(_)
        | vn_core::TypeKind::This
        | vn_core::TypeKind::Array(_)
        | vn_core::TypeKind::Union(_)
        | vn_core::TypeKind::Intersection(_)
        | vn_core::TypeKind::Tuple(_)
        | vn_core::TypeKind::Named(_, _)
        | vn_core::TypeKind::TemplateLiteral(_)
        | vn_core::TypeKind::Fn(_)
        | vn_core::TypeKind::Record(_)
        | vn_core::TypeKind::Interface(_)
        | vn_core::TypeKind::Typeof(_)
        | vn_core::TypeKind::KeyOf(_)
        | vn_core::TypeKind::IndexedAccess { .. }
        | vn_core::TypeKind::Mapped { .. }
        | vn_core::TypeKind::Conditional { .. }
        | vn_core::TypeKind::Infer(_)
        | vn_core::TypeKind::EnumVariant { .. }
        | vn_core::TypeKind::TypePredicate { .. } => String::new(),
    }
}

pub fn format_signature(state: &DocumentState, sym: SymbolView<'_>) -> String {
    match sym.kind() {
        SymbolKind::Function | SymbolKind::Method => format_fn(sym),
        SymbolKind::Class => format_class(state, sym),
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
    vn_core::LangPrimitive::from_str(name).is_some()
}

fn format_class(state: &DocumentState, sym: SymbolView<'_>) -> String {
    let tp = format_type_params(&sym.type_params());
    let members = state.members_of(sym);
    if members.is_empty() || is_primitive_class_name(sym.name()) {
        return format!("class {}{}", sym.name(), tp);
    }
    let mut lines = vec![format!("class {}{} {{", sym.name(), tp)];
    for m in &members {
        lines.push(format_summary_member(state, m));
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
        lines.push(format_summary_member(state, m));
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
        lines.push(format_summary_member(state, m));
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

pub fn format_summary_member(state: &DocumentState, m: &ResolvedMemberSummary) -> String {
    let indent = "  ";
    let static_prefix = if m.is_static { "static " } else { "" };
    let optional = if m.optional { "?" } else { "" };

    let shape = state.db.fn_shape(&m.ty);
    match shape {
        Some(ft) if !matches!(m.kind, ResolvedMemberKind::Getter) => {
            let params = ft
                .params
                .iter()
                .map(|p| {
                    format!(
                        "{}{}{}: {}",
                        if p.is_rest { "..." } else { "" },
                        p.name.as_deref().unwrap_or("arg"),
                        if p.optional { "?" } else { "" },
                        state.db.id_text(p.ty),
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "{indent}{static_prefix}{}{optional}({params}): {}",
                m.name,
                state.db.id_text(ft.return_type)
            )
        }
        None | Some(_) => match m.kind {
            ResolvedMemberKind::Getter => {
                let ty = state.ty_text(&m.ty);
                format!("{indent}{static_prefix}get {}(): {ty}", m.name)
            }
            ResolvedMemberKind::Setter => {
                let ty = state.ty_text(&m.ty);
                format!("{indent}{static_prefix}set {}({ty})", m.name)
            }
            ResolvedMemberKind::EnumMember => format!("{indent}{}", m.name),
            ResolvedMemberKind::Method
            | ResolvedMemberKind::Property
            | ResolvedMemberKind::StaticMethod
            | ResolvedMemberKind::StaticProperty
            | ResolvedMemberKind::ExtensionMethod
            | ResolvedMemberKind::ExtensionProperty
            | ResolvedMemberKind::NestedType(_)
            | ResolvedMemberKind::Constructor => format!(
                "{indent}{static_prefix}{}{optional}: {}",
                m.name,
                state.ty_text(&m.ty)
            ),
        },
    }
}
