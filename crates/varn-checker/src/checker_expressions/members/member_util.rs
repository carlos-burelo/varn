use crate::binder::BindResult;
use crate::types::{CheckerTyTable, Type};
use rustc_hash::FxHashMap;
use std::sync::Arc;
use varn_core::TypeKind;

pub(super) fn class_type_params(
    resolver: &dyn crate::module_resolver::ImportResolver,
    name: &str,
    origin: Option<&Arc<str>>,
    bind: &BindResult,
) -> Vec<Arc<str>> {
    let local = params_in(name, bind);
    if !local.is_empty() {
        return local;
    }
    if let Some(params) = bind
        .core
        .as_ref()
        .and_then(|b| b.class_type_params.get(name))
    {
        return params.clone();
    }
    let origins: Vec<String> = origin.iter().map(|s| s.to_string()).collect();
    if let Some(b) = resolver.find_bind_for_type(name, &origins) {
        return params_in(name, &b);
    }
    if origin.is_none() {
        for spec in varn_modules::std_module_ids() {
            if let Some(b) = resolver.stdlib_bind(spec) {
                let params = params_in(name, &b);
                if !params.is_empty() {
                    return params;
                }
            }
        }
    }
    Vec::new()
}

fn params_in(name: &str, bind: &BindResult) -> Vec<Arc<str>> {
    bind.interner
        .get(name)
        .and_then(|atom| {
            bind.scopes
                .get(bind.global_scope)
                .resolve(atom, &bind.scopes)
        })
        .map(|sid| {
            bind.arena
                .get(sid)
                .type_params
                .iter()
                .map(|a| Arc::from(bind.interner.resolve(*a)))
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn generic_mapping(
    resolver: &dyn crate::module_resolver::ImportResolver,
    name: &str,
    args: &[Type],
    origin: Option<&Arc<str>>,
    bind: &BindResult,
) -> FxHashMap<varn_core::Atom, Type> {
    if args.is_empty() {
        return FxHashMap::default();
    }
    class_type_params(resolver, name, origin, bind)
        .into_iter()
        .map(|p| varn_core::Atom::of(&p))
        .zip(args.iter().cloned())
        .collect()
}

fn resolve_extension_symbol_type(bind: &BindResult, mangled: &Arc<str>) -> Option<Type> {
    let scope = bind.scopes.get(bind.global_scope);
    let atom = bind.interner.get(mangled.as_ref())?;
    let sid = scope.resolve(atom, &bind.scopes)?;
    bind.arena.get(sid).ty
}

pub(super) fn extension_method_type(
    bind: &BindResult,
    mangled: &Arc<str>,
    table: &mut CheckerTyTable,
) -> Option<Type> {
    let sym_ty = resolve_extension_symbol_type(bind, mangled)?;
    let TypeKind::Fn(fid) = table.get(sym_ty.0) else {
        return Some(sym_ty);
    };
    let ft = table.get_function(fid).clone();

    let params: Vec<crate::types::FunctionParam> = ft
        .params
        .iter()
        .skip_while(|p| p.name.as_deref() == Some("this"))
        .cloned()
        .collect();

    Some(Type::fn_(
        crate::types::FunctionType {
            params,
            return_type: ft.return_type,
            is_arrow: ft.is_arrow,
            type_params: ft.type_params.clone(),
        },
        table,
    ))
}

pub(super) fn extension_getter_type(
    bind: &BindResult,
    mangled: &Arc<str>,
    table: &CheckerTyTable,
) -> Option<Type> {
    let sym_ty = resolve_extension_symbol_type(bind, mangled)?;
    match table.get(sym_ty.0) {
        TypeKind::Fn(fid) => Some(Type::resolved(table.get_function(fid).return_type)),
        TypeKind::Primitive(_) | TypeKind::Builtin(_) | TypeKind::Literal(_) | TypeKind::This | TypeKind::Array(_) | TypeKind::Union(_) | TypeKind::Intersection(_) | TypeKind::Tuple(_) | TypeKind::Named(..) | TypeKind::Generic(..) | TypeKind::TemplateLiteral(_) | TypeKind::Object(_) | TypeKind::Typeof(_) | TypeKind::KeyOf(_) | TypeKind::IndexedAccess { .. } | TypeKind::Mapped { .. } | TypeKind::Conditional { .. } | TypeKind::Infer(_) | TypeKind::EnumVariant { .. } | TypeKind::TypePredicate { .. } => Some(sym_ty),
    }
}

pub(super) fn intrinsic_member_info(
    bind: &BindResult,
    type_name: &str,
    key: &str,
) -> Option<(Type, Option<usize>)> {
    bind.core
        .as_ref()
        .and_then(|b| b.class_members.get(type_name))
        .and_then(|members| {
            members
                .members
                .iter()
                .find(|m| m.name.as_ref() == key)
                .map(|m| (m.ty, m.symbol_id))
        })
}
