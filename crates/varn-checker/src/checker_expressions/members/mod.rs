mod member_exists;
pub(crate) mod member_type;

use std::sync::Arc;

use crate::binder::BindResult;
use crate::checker::Checker;
use crate::types::{CheckerTyTable, ObjectTypeMember, Type};
use varn_core::TypeKind;

/// Is `name` an enum, as `bind` sees it?
///
/// The origin module is consulted last, and only when the local tables cannot
/// settle the question. They almost always can, and the ordering matters more
/// than it looks: resolving `origin` means binding a module, and a type
/// declared in the file under check carries that file as its origin -- so the
/// probe re-bound the very `BindResult` already passed in as `bind`. Two calls
/// per member access (`find_member_info_uncached` and `member_exists`) put a
/// full read-parse-bind of the current file on the path of every `p.x`.
///
/// A name in `type_members.classes` but not in `type_members.enums` is a class
/// or struct: enums are written to both, so the local-enum check above has
/// already returned for them.
pub(crate) fn is_enum_type(
    resolver: &dyn crate::module_resolver::ImportResolver,
    bind: &BindResult,
    name: &Arc<str>,
    origin_modules: &[String],
) -> bool {
    if bind.get_enum_members_local(name.as_ref()).is_some() {
        return true;
    }
    if bind
        .core
        .as_ref()
        .is_some_and(|b| b.enum_members.contains_key(name.as_ref()))
    {
        return true;
    }
    if bind.type_members.classes.contains_key(name)
        || bind.type_members.interfaces.contains_key(name)
    {
        return false;
    }
    resolver
        .find_bind_for_type(name, origin_modules)
        .as_ref()
        .is_some_and(|eb| eb.get_enum_members_local(name.as_ref()).is_some())
}

fn nested(k: crate::semantic_info::NestedTypeKind) -> crate::semantic_info::ResolvedMemberKind {
    crate::semantic_info::ResolvedMemberKind::NestedType(k)
}

fn map_class_member_kind(
    k: crate::binder::ClassMemberKind,
) -> crate::semantic_info::ResolvedMemberKind {
    match k {
        crate::binder::ClassMemberKind::Method | crate::binder::ClassMemberKind::Function => {
            crate::semantic_info::ResolvedMemberKind::Method
        }
        crate::binder::ClassMemberKind::Property | crate::binder::ClassMemberKind::Variable => {
            crate::semantic_info::ResolvedMemberKind::Property
        }
        crate::binder::ClassMemberKind::Constructor => {
            crate::semantic_info::ResolvedMemberKind::Constructor
        }
        crate::binder::ClassMemberKind::Getter => crate::semantic_info::ResolvedMemberKind::Getter,
        crate::binder::ClassMemberKind::Setter => crate::semantic_info::ResolvedMemberKind::Setter,
        crate::binder::ClassMemberKind::Class => {
            nested(crate::semantic_info::NestedTypeKind::Class)
        }
        crate::binder::ClassMemberKind::Interface => {
            nested(crate::semantic_info::NestedTypeKind::Interface)
        }
        crate::binder::ClassMemberKind::Namespace => {
            nested(crate::semantic_info::NestedTypeKind::Namespace)
        }
        crate::binder::ClassMemberKind::Enum => nested(crate::semantic_info::NestedTypeKind::Enum),
        crate::binder::ClassMemberKind::Struct => {
            nested(crate::semantic_info::NestedTypeKind::Struct)
        }
    }
}

/// Every member reachable on `ty`, including those declared in other modules.
///
/// `resolver` is what makes the cross-module half possible; without it this
/// could only answer for types declared locally.
pub fn get_members_of_type(
    resolver: &dyn crate::module_resolver::ImportResolver,
    ty: &Type,
    bind: &BindResult,
    table: &mut CheckerTyTable,
) -> Vec<crate::semantic_info::ResolvedMemberSummary> {
    let mut results: Vec<crate::semantic_info::ResolvedMemberSummary> = Vec::new();
    let mut seen = rustc_hash::FxHashSet::default();

    // `at` is the declaration site: `None` for members with no source of
    // their own (interface blobs, tuple indices, intrinsic properties).
    let add_member = |results: &mut Vec<crate::semantic_info::ResolvedMemberSummary>,
                      seen: &mut rustc_hash::FxHashSet<Arc<str>>,
                      name: Arc<str>,
                      ty: Type,
                      kind: crate::semantic_info::ResolvedMemberKind,
                      is_static: bool,
                      optional: bool,
                      readonly: bool| {
        if seen.insert(name.clone()) {
            results.push(crate::semantic_info::ResolvedMemberSummary {
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

    /// `add_member` for members that come from a `ClassMemberInfo`, which
    /// carries the declaration site the editor needs.
    fn add_declared(
        results: &mut Vec<crate::semantic_info::ResolvedMemberSummary>,
        seen: &mut rustc_hash::FxHashSet<Arc<str>>,
        m: &crate::types::ClassMemberInfo,
        ty: Type,
        kind: crate::semantic_info::ResolvedMemberKind,
    ) {
        if seen.insert(m.name.clone()) {
            results.push(crate::semantic_info::ResolvedMemberSummary {
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
                            Type(ty, false),
                            crate::semantic_info::ResolvedMemberKind::Property,
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
                        ..
                    } => {
                        let fn_ty = Type::fn_(
                            crate::types::FunctionType {
                                params: params.clone(),
                                return_type,
                                is_arrow,
                                type_params: vec![],
                            },
                            table,
                        );
                        add_member(
                            &mut results,
                            &mut seen,
                            name.clone(),
                            fn_ty,
                            crate::semantic_info::ResolvedMemberKind::Method,
                            false,
                            false,
                            true,
                        );
                    }
                    _ => {}
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
                    Type(*elem, false),
                    crate::semantic_info::ResolvedMemberKind::Property,
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
                crate::semantic_info::ResolvedMemberKind::Property,
                false,
                false,
                true,
            );
        }
        TypeKind::Array(inner) => {
            let atom = resolver.intern(varn_core::BuiltinType::Array.name());
            let array_ty = Type::generic_atom(atom, vec![Type(inner, false)], None, table);
            return get_members_of_type(resolver, &array_ty, bind, table);
        }
        TypeKind::Named(cn_atom, origin_atom) | TypeKind::Generic(cn_atom, _, origin_atom) => {
            let cn: Arc<str> = Arc::from(bind.interner.resolve(cn_atom));
            let origin: Option<Arc<str>> = origin_atom.map(|o| Arc::from(bind.interner.resolve(o)));
            let mapping = if let TypeKind::Generic(_, args_list, _) = ty_kind {
                let args: Vec<Type> = table
                    .get_list(args_list)
                    .iter()
                    .map(|id| Type(*id, false))
                    .collect();
                member_type::generic_mapping(resolver, cn.as_ref(), &args, origin.as_ref(), bind)
            } else {
                rustc_hash::FxHashMap::default()
            };

            let map_ty = |t: &Type, table: &mut CheckerTyTable| {
                if mapping.is_empty() {
                    *t
                } else {
                    t.map_generics(&mapping, table)
                }
            };

            // 1. Check local type_members
            if let Some(entry) = bind.type_members.classes.get(&cn) {
                for m in &entry.members {
                    let kind = map_class_member_kind(m.kind);
                    let mapped = map_ty(&m.ty, table);
                    add_declared(&mut results, &mut seen, m, mapped, kind);
                }
            }
            if let Some(entry) = bind.type_members.interfaces.get(&cn) {
                for m in entry {
                    let kind = map_class_member_kind(m.kind);
                    let mapped = map_ty(&m.ty, table);
                    add_declared(&mut results, &mut seen, m, mapped, kind);
                }
            }

            // 2. Check core members
            if let Some(b) = &bind.core {
                if let Some(entry) = b.class_members.get(cn.as_ref()) {
                    for m in &entry.members {
                        let kind = map_class_member_kind(m.kind);
                        let mapped = map_ty(&m.ty, table);
                        add_declared(&mut results, &mut seen, m, mapped, kind);
                    }
                }
                if let Some(members) = b.flattened_members.get(cn.as_ref()) {
                    for m in members {
                        let kind = map_class_member_kind(m.kind);
                        let mapped = map_ty(&m.ty, table);
                        add_declared(&mut results, &mut seen, m, mapped, kind);
                    }
                }
            }

            // 3. Check external / stdlib module binds
            let origin_modules: Vec<String> = origin.iter().map(|s| s.to_string()).collect();
            if let Some(ext_bind) = resolver.find_bind_for_type(&cn, &origin_modules) {
                if let Some(entry) = ext_bind.type_members.classes.get(&cn) {
                    for m in &entry.members {
                        let kind = map_class_member_kind(m.kind);
                        let mapped = map_ty(&m.ty, table);
                        add_declared(&mut results, &mut seen, m, mapped, kind);
                    }
                }
            }
        }
        TypeKind::Primitive(varn_core::LangPrimitive::Str) => {
            add_member(
                &mut results,
                &mut seen,
                Arc::from(varn_core::MemberKey::Length.as_str()),
                Type::Int,
                crate::semantic_info::ResolvedMemberKind::Property,
                false,
                false,
                true,
            );
            let str_ty = Type::named(varn_core::RuntimeKind::Str.name().to_owned(), resolver, table);
            return get_members_of_type(resolver, &str_ty, bind, table);
        }
        TypeKind::Builtin(varn_core::BuiltinType::Bytes) => {
            add_member(
                &mut results,
                &mut seen,
                Arc::from(varn_core::MemberKey::Length.as_str()),
                Type::Int,
                crate::semantic_info::ResolvedMemberKind::Property,
                false,
                false,
                true,
            );
            let bytes_ty =
                Type::named(varn_core::RuntimeKind::Bytes.name().to_owned(), resolver, table);
            return get_members_of_type(resolver, &bytes_ty, bind, table);
        }
        kind @ (TypeKind::Primitive(_) | TypeKind::Builtin(_) | TypeKind::Literal(_)) => {
            let name = kind.lang_name().unwrap_or_default();
            let named_ty = Type::named(name.to_owned(), resolver, table);
            return get_members_of_type(resolver, &named_ty, bind, table);
        }
        _ => {}
    }

    collect_extension_members(&mut results, &mut seen, ty, bind, table);
    results
}

/// Append the extension methods, getters and setters declared for `ty`.
///
/// Without this, `get_members_of_type` did not live up to its own contract —
/// "every member reachable on `ty`" — because an `extension` block declares
/// members that are as reachable as any other. The language server kept its own
/// table to fill the gap, so extensions had a second, parallel definition that
/// only tooling could see.
fn collect_extension_members(
    results: &mut Vec<crate::semantic_info::ResolvedMemberSummary>,
    seen: &mut rustc_hash::FxHashSet<Arc<str>>,
    ty: &Type,
    bind: &BindResult,
    table: &mut CheckerTyTable,
) {
    let Some(type_name) = extension_key(ty, table, &bind.interner) else {
        return;
    };
    let scope = bind.scopes.get(bind.global_scope);

    // Extension bodies take the receiver as a leading `this` parameter. It is an
    // implementation detail of the lowering, not part of the member's signature.
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
        let TypeKind::Fn(fid) = table.get(sym_ty.0) else {
            return;
        };
        let ft = table.get_function(fid).clone();
        // A getter reads as its return type; a method reads as its signature.
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

/// The name `extension` blocks are keyed by for `ty`.
fn extension_key(
    ty: &Type,
    table: &CheckerTyTable,
    interner: &varn_core::AtomInterner,
) -> Option<Arc<str>> {
    match table.get(ty.0) {
        TypeKind::Named(n, _) | TypeKind::Generic(n, _, _) => Some(Arc::from(interner.resolve(n))),
        k @ (TypeKind::Primitive(_) | TypeKind::Builtin(_) | TypeKind::Literal(_)) => k.lang_name().map(Arc::from),
        TypeKind::Array(_) => Some(Arc::from(varn_core::BuiltinType::Array.name())),
        _ => None,
    }
}

impl<'r> Checker<'r> {
    pub(crate) fn collect_member_names(&self, ty: &Type, bind: &BindResult) -> Vec<Arc<str>> {
        let ty_kind = self.ty_table.get(ty.0);
        match ty_kind {
            TypeKind::Object(mid) => self
                .ty_table
                .get_object_members(mid)
                .iter()
                .filter_map(|m| match m {
                    ObjectTypeMember::Property { name, .. }
                    | ObjectTypeMember::Method { name, .. } => Some(name.clone()),
                    _ => None,
                })
                .collect(),
            TypeKind::Named(cn, _) | TypeKind::Generic(cn, _, _) => {
                let cn_str = bind.interner.resolve(cn);
                bind.get_class_entry(cn_str)
                    .map(|entry| entry.members.iter().map(|m| m.name.clone()).collect())
                    .unwrap_or_default()
            }
            TypeKind::Union(list) => {
                let mut names: Vec<Arc<str>> = Vec::new();
                for id in self.ty_table.get_list(list).to_vec() {
                    let m = Type(id, false);
                    if !m.is_nullable(&self.ty_table) {
                        names.extend(self.collect_member_names(&m, bind));
                    }
                }
                names.sort_unstable();
                names.dedup();
                names
            }
            _ => Vec::new(),
        }
    }
}
