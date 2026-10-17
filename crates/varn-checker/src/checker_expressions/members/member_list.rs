use std::sync::Arc;

use varn_core::TypeKind;
use varn_sem::bind::BindResult;
use varn_sem::types::{CheckerTyTable, ObjectTypeMember, Type};

pub fn get_members_of_type(
    resolver: &dyn varn_sem::resolver::ImportResolver,
    ty: &Type,
    bind: &BindResult,
    table: &mut CheckerTyTable,
) -> Vec<varn_sem::semantic_info::ResolvedMemberSummary> {
    let mut results: Vec<varn_sem::semantic_info::ResolvedMemberSummary> = Vec::new();
    let mut seen = rustc_hash::FxHashSet::default();

    let add_member = |results: &mut Vec<varn_sem::semantic_info::ResolvedMemberSummary>,
                      seen: &mut rustc_hash::FxHashSet<Arc<str>>,
                      name: Arc<str>,
                      ty: Type,
                      kind: varn_sem::semantic_info::ResolvedMemberKind,
                      is_static: bool,
                      optional: bool,
                      readonly: bool| {
        if seen.insert(name.clone()) {
            results.push(varn_sem::semantic_info::ResolvedMemberSummary {
                name,
                ty,
                kind,
                is_static,
                optional,
                readonly,
                def_line: None,
                def_col: 0,
                is_async: false,
                is_generator: false,
            });
        }
    };

    let ty_kind = table.get(ty.0);
    match ty_kind {
        TypeKind::Object(mid) => {
            for m in table.get_object_members(mid).to_vec() {
                match m {
                    ObjectTypeMember::Property {
                        name,
                        ty,
                        optional,
                        readonly,
                    } => {
                        add_member(
                            &mut results,
                            &mut seen,
                            name.clone(),
                            Type::resolved(ty),
                            varn_sem::semantic_info::ResolvedMemberKind::Property,
                            false,
                            optional,
                            readonly,
                        );
                    }
                    ObjectTypeMember::Method {
                        name,
                        params,
                        return_type,
                        is_arrow,
                        type_params,
                        ..
                    } => {
                        let fn_ty = Type::fn_(
                            varn_sem::types::FunctionType {
                                params: params.clone(),
                                return_type,
                                is_arrow,
                                type_params: type_params.clone(),
                            },
                            table,
                        );
                        add_member(
                            &mut results,
                            &mut seen,
                            name.clone(),
                            fn_ty,
                            varn_sem::semantic_info::ResolvedMemberKind::Method,
                            false,
                            false,
                            true,
                        );
                    }
                    ObjectTypeMember::Index { .. } | ObjectTypeMember::Callable { .. } => {}
                }
            }
        }
        TypeKind::Tuple(list) => {
            let elems = table.get_list(list).to_vec();
            for (idx, elem) in elems.iter().enumerate() {
                add_member(
                    &mut results,
                    &mut seen,
                    Arc::from(idx.to_string()),
                    Type::resolved(*elem),
                    varn_sem::semantic_info::ResolvedMemberKind::Property,
                    false,
                    false,
                    false,
                );
            }
            add_member(
                &mut results,
                &mut seen,
                Arc::from("length"),
                Type::Int,
                varn_sem::semantic_info::ResolvedMemberKind::Property,
                false,
                false,
                true,
            );
        }
        TypeKind::Array(inner) => list_nominal(
            resolver,
            bind,
            table,
            Arc::from(varn_core::BuiltinType::Array.name()),
            Some(Arc::from(varn_core::BuiltinType::Array.core_module())),
            &[Type::resolved(inner)],
            &mut results,
            &mut seen,
        ),
        TypeKind::Named(cn_atom, origin_atom) | TypeKind::Generic(cn_atom, _, origin_atom) => {
            let cn: Arc<str> = Arc::from(super::member_atom::resolve_atom_text(bind, cn_atom));
            let origin: Option<Arc<str>> =
                origin_atom.map(|o| Arc::from(super::member_atom::resolve_atom_text(bind, o)));
            let args: Vec<Type> = if let TypeKind::Generic(_, args_list, _) = ty_kind {
                table
                    .get_list(args_list)
                    .iter()
                    .map(|id| Type::resolved(*id))
                    .collect()
            } else {
                Vec::new()
            };
            list_nominal(
                resolver,
                bind,
                table,
                cn,
                origin,
                &args,
                &mut results,
                &mut seen,
            );
        }
        TypeKind::Primitive(varn_core::LangPrimitive::Str) => {
            add_member(
                &mut results,
                &mut seen,
                Arc::from(varn_core::MemberKey::Length.as_str()),
                Type::Int,
                varn_sem::semantic_info::ResolvedMemberKind::Property,
                false,
                false,
                true,
            );
            list_primitive(
                resolver,
                bind,
                table,
                varn_core::LangPrimitive::Str,
                &mut results,
                &mut seen,
            );
        }
        TypeKind::Primitive(p) => {
            list_primitive(resolver, bind, table, p, &mut results, &mut seen);
        }
        TypeKind::Literal(l) => {
            list_primitive(resolver, bind, table, l.base(), &mut results, &mut seen);
        }
        TypeKind::Union(list) | TypeKind::Intersection(list) => {
            let variants: Vec<Type> = table
                .get_list(list)
                .to_vec()
                .iter()
                .map(|id| Type::resolved(*id))
                .collect();
            let mut per_variant = Vec::with_capacity(variants.len());
            for v in &variants {
                if matches!(
                    table.get(v.0),
                    TypeKind::Primitive(varn_core::LangPrimitive::Dynamic)
                ) {
                    continue;
                }
                per_variant.push(get_members_of_type(resolver, v, bind, table));
            }
            if let Some((first, rest)) = per_variant.split_first() {
                for m in first {
                    if rest.iter().all(|o| o.iter().any(|x| x.name == m.name))
                        && seen.insert(m.name.clone())
                    {
                        results.push(m.clone());
                    }
                }
            }
        }
        TypeKind::This
        | TypeKind::TemplateLiteral(_)
        | TypeKind::Fn(_)
        | TypeKind::Typeof(_)
        | TypeKind::KeyOf(_)
        | TypeKind::IndexedAccess { .. }
        | TypeKind::Mapped { .. }
        | TypeKind::Conditional { .. }
        | TypeKind::Infer(_)
        | TypeKind::EnumVariant { .. }
        | TypeKind::TypePredicate { .. } => {}
    }

    super::member_extension::collect_extension_members(&mut results, &mut seen, ty, bind, table);
    results
}

#[allow(clippy::too_many_arguments)]
fn list_nominal(
    resolver: &dyn varn_sem::resolver::ImportResolver,
    bind: &BindResult,
    table: &mut CheckerTyTable,
    cn: Arc<str>,
    origin: Option<Arc<str>>,
    args: &[Type],
    results: &mut Vec<varn_sem::semantic_info::ResolvedMemberSummary>,
    seen: &mut rustc_hash::FxHashSet<Arc<str>>,
) {
    let mapping = if args.is_empty() {
        rustc_hash::FxHashMap::default()
    } else {
        super::member_util::generic_mapping(resolver, cn.as_ref(), args, origin.as_ref(), bind)
    };

    let map_ty = |t: &Type, table: &mut CheckerTyTable| {
        if mapping.is_empty() {
            *t
        } else {
            t.map_generics(&mapping, table)
        }
    };

    if let Some(entry) = bind.type_members.classes.get(&cn) {
        for m in &entry.members {
            let kind = super::member_kind::map_class_member_kind(m.kind);
            let mapped = map_ty(&m.ty, table);
            add_declared(results, seen, m, mapped, kind);
        }
    }
    if let Some(entry) = bind.type_members.interfaces.get(&cn) {
        for m in entry {
            let kind = super::member_kind::map_class_member_kind(m.kind);
            let mapped = map_ty(&m.ty, table);
            add_declared(results, seen, m, mapped, kind);
        }
    }
    if let Some(entry) = bind.type_members.enums.get(&cn) {
        for m in entry {
            let kind = super::member_kind::map_class_member_kind(m.kind);
            let mapped = map_ty(&m.ty, table);
            add_declared(results, seen, m, mapped, kind);
        }
    }
    if let Some(entry) = bind.type_members.namespaces.get(&cn) {
        for m in entry {
            let kind = super::member_kind::map_class_member_kind(m.kind);
            let mapped = map_ty(&m.ty, table);
            add_declared(results, seen, m, mapped, kind);
        }
    }

    if let Some(b) = &bind.core {
        if let Some(entry) = b.class_members.get(cn.as_ref()) {
            for m in &entry.members {
                let kind = super::member_kind::map_class_member_kind(m.kind);
                let mapped = map_ty(&m.ty, table);
                add_declared(results, seen, m, mapped, kind);
            }
        }
        if let Some(members) = b.flattened_members.get(cn.as_ref()) {
            for m in members {
                let kind = super::member_kind::map_class_member_kind(m.kind);
                let mapped = map_ty(&m.ty, table);
                add_declared(results, seen, m, mapped, kind);
            }
        }
    }

    let origin_modules: Vec<String> = origin.iter().map(|s| s.to_string()).collect();
    if let Some(ext_bind) = resolver.find_bind_for_type(&cn, &origin_modules)
        && let Some(entry) = ext_bind.type_members.classes.get(&cn)
    {
        for m in &entry.members {
            let kind = super::member_kind::map_class_member_kind(m.kind);
            let mapped = map_ty(&m.ty, table);
            add_declared(results, seen, m, mapped, kind);
        }
    }
}

fn add_declared(
    results: &mut Vec<varn_sem::semantic_info::ResolvedMemberSummary>,
    seen: &mut rustc_hash::FxHashSet<Arc<str>>,
    m: &varn_sem::types::ClassMemberInfo,
    ty: Type,
    kind: varn_sem::semantic_info::ResolvedMemberKind,
) {
    if seen.insert(m.name.clone()) {
        results.push(varn_sem::semantic_info::ResolvedMemberSummary {
            name: m.name.clone(),
            ty,
            kind,
            is_static: m.is_static,
            optional: m.is_optional,
            readonly: m.is_readonly,
            def_line: (m.line > 0).then_some(m.line),
            def_col: m.col,
            is_async: m.is_async,
            is_generator: m.is_generator,
        });
    }
}

fn list_primitive(
    resolver: &dyn varn_sem::resolver::ImportResolver,
    bind: &BindResult,
    table: &mut CheckerTyTable,
    primitive: varn_core::LangPrimitive,
    results: &mut Vec<varn_sem::semantic_info::ResolvedMemberSummary>,
    seen: &mut rustc_hash::FxHashSet<Arc<str>>,
) {
    if let Some(module) = primitive.core_module() {
        let name = Arc::from(primitive.name());
        list_nominal(
            resolver,
            bind,
            table,
            name,
            Some(Arc::from(module)),
            &[],
            results,
            seen,
        );
    }
}
