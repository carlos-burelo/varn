use std::sync::Arc;

use crate::binder::BindResult;
use crate::types::{CheckerTyTable, Type};

pub(super) fn collect_extension_members(
    resolver: &dyn crate::module_resolver::ImportResolver,
    results: &mut Vec<crate::semantic_info::ResolvedMemberSummary>,
    seen: &mut rustc_hash::FxHashSet<Arc<str>>,
    ty: &Type,
    bind: &BindResult,
    table: &mut CheckerTyTable,
) {
    let Some(type_name) = extension_key(resolver, ty, table, bind) else {
        return;
    };
    let scope = bind.scopes.get(bind.global_scope);

    let strip_this = |ft: &crate::types::FunctionType| {
        let mut params = ft.params.clone();
        if params.first().and_then(|p| p.name.as_deref()) == Some("this") {
            params.remove(0);
        }
        crate::types::FunctionType {
            params,
            return_type: ft.return_type,
            is_arrow: ft.is_arrow,
            type_params: ft.type_params.clone(),
        }
    };

    let push = |name: &Arc<str>,
                mangled: &Arc<str>,
                kind: crate::semantic_info::ResolvedMemberKind,
                as_return: bool,
                results: &mut Vec<crate::semantic_info::ResolvedMemberSummary>,
                seen: &mut rustc_hash::FxHashSet<Arc<str>>,
                table: &mut CheckerTyTable| {
        let Some(sid) = bind
            .interner
            .get(mangled.as_ref())
            .and_then(|atom| scope.resolve(atom, &bind.scopes))
        else {
            return;
        };
        let sym = bind.arena.get(sid);
        let Some(sym_ty) = &sym.ty else {
            return;
        };
        let varn_core::TypeKind::Fn(fid) = table.get(sym_ty.0) else {
            return;
        };
        let ft = table.get_function(fid).clone();
        let member_ty = if as_return {
            Type(ft.return_type, false)
        } else {
            Type::fn_(strip_this(&ft), table)
        };
        if seen.insert(name.clone()) {
            results.push(crate::semantic_info::ResolvedMemberSummary {
                name: name.clone(),
                ty: member_ty,
                kind,
                is_static: false,
                optional: false,
                readonly: false,
                def_line: (sym.line > 0).then_some(sym.line),
                def_col: sym.col,
                is_async: sym.is_async,
                is_generator: sym.is_generator,
            });
        }
    };

    use crate::semantic_info::ResolvedMemberKind as K;
    if let Some(methods) = bind.extensions.methods.get(type_name.as_ref()) {
        for (name, mangled) in methods {
            push(
                name,
                mangled,
                K::ExtensionMethod,
                false,
                results,
                seen,
                table,
            );
        }
    }
    if let Some(getters) = bind.extensions.getters.get(type_name.as_ref()) {
        for (name, mangled) in getters {
            push(
                name,
                mangled,
                K::ExtensionProperty,
                true,
                results,
                seen,
                table,
            );
        }
    }
    if let Some(setters) = bind.extensions.setters.get(type_name.as_ref()) {
        for (name, mangled) in setters {
            push(
                name,
                mangled,
                K::ExtensionProperty,
                true,
                results,
                seen,
                table,
            );
        }
    }
}

fn extension_key(
    resolver: &dyn crate::module_resolver::ImportResolver,
    ty: &Type,
    table: &CheckerTyTable,
    bind: &BindResult,
) -> Option<Arc<str>> {
    match table.get(ty.0) {
        varn_core::TypeKind::Named(n, _) | varn_core::TypeKind::Generic(n, _, _) => {
            Some(Arc::from(super::member_atom::resolve_atom_text(
                resolver, bind, n,
            )))
        }
        k @ (varn_core::TypeKind::Primitive(_) | varn_core::TypeKind::Builtin(_) | varn_core::TypeKind::Literal(_)) => {
            k.lang_name().map(Arc::from)
        }
        varn_core::TypeKind::Array(_) => Some(Arc::from(varn_core::BuiltinType::Array.name())),
        _ => None,
    }
}
