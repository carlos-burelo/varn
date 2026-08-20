mod member_exists;
mod member_type;

use std::rc::Rc;

use crate::binder::BindResult;
use crate::checker::Checker;
use crate::types::{ObjectTypeMember, Type};
use varn_core::TypeKind;

fn map_class_member_kind(k: crate::binder::ClassMemberKind) -> crate::semantic_info::ResolvedMemberKind {
    match k {
        crate::binder::ClassMemberKind::Method | crate::binder::ClassMemberKind::Function => {
            crate::semantic_info::ResolvedMemberKind::Method
        }
        crate::binder::ClassMemberKind::Property
        | crate::binder::ClassMemberKind::Variable
        | crate::binder::ClassMemberKind::Constructor => {
            crate::semantic_info::ResolvedMemberKind::Property
        }
        crate::binder::ClassMemberKind::Getter => crate::semantic_info::ResolvedMemberKind::Getter,
        crate::binder::ClassMemberKind::Setter => crate::semantic_info::ResolvedMemberKind::Setter,
        crate::binder::ClassMemberKind::Class
        | crate::binder::ClassMemberKind::Interface
        | crate::binder::ClassMemberKind::Namespace
        | crate::binder::ClassMemberKind::Enum
        | crate::binder::ClassMemberKind::Struct => {
            crate::semantic_info::ResolvedMemberKind::Property
        }
    }
}

pub fn get_members_of_type(
    ty: &Type,
    bind: &BindResult,
) -> Vec<crate::semantic_info::ResolvedMemberSummary> {
        let mut results: Vec<crate::semantic_info::ResolvedMemberSummary> = Vec::new();
        let mut seen = rustc_hash::FxHashSet::default();

        let add_member = |results: &mut Vec<crate::semantic_info::ResolvedMemberSummary>,
                          seen: &mut rustc_hash::FxHashSet<Rc<str>>,
                          name: Rc<str>,
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
                });
            }
        };

        match &ty.0 {
            TypeKind::Object(members) => {
                for m in members {
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
                                ty.clone(),
                                crate::semantic_info::ResolvedMemberKind::Property,
                                false,
                                *optional,
                                *readonly,
                            );
                        }
                        ObjectTypeMember::Method {
                            name,
                            params,
                            return_type,
                            is_arrow,
                            ..
                        } => {
                            let fn_ty = Type::fn_(crate::types::FunctionType {
                                params: params.clone(),
                                return_type: return_type.clone(),
                                is_arrow: *is_arrow,
                                type_params: vec![],
                            });
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
            TypeKind::Tuple(elems) => {
                for (idx, elem) in elems.iter().enumerate() {
                    add_member(
                        &mut results,
                        &mut seen,
                        Rc::from(idx.to_string()),
                        elem.clone(),
                        crate::semantic_info::ResolvedMemberKind::Property,
                        false,
                        false,
                        false,
                    );
                }
                add_member(
                    &mut results,
                    &mut seen,
                    Rc::from("length"),
                    Type::Int,
                    crate::semantic_info::ResolvedMemberKind::Property,
                    false,
                    false,
                    true,
                );
            }
            TypeKind::Array(inner) => {
                let array_ty = Type::generic(
                    varn_core::IntrinsicType::Array.as_str().to_owned(),
                    vec![*inner.clone()],
                );
                return get_members_of_type(&array_ty, bind);
            }
            TypeKind::Named(cn, origin) | TypeKind::Generic(cn, _, origin) => {
                let mapping = if let TypeKind::Generic(_, args, _) = &ty.0 {
                    member_type::generic_mapping(cn.as_ref(), args, origin.as_ref(), bind)
                } else {
                    rustc_hash::FxHashMap::default()
                };

                let map_ty = |t: &Type| {
                    if mapping.is_empty() {
                        t.clone()
                    } else {
                        t.map_generics(&mapping)
                    }
                };

                // 1. Check local type_members
                if let Some(entry) = bind.type_members.classes.get(cn) {
                    for m in &entry.members {
                        let kind = map_class_member_kind(m.kind);
                        add_member(
                            &mut results,
                            &mut seen,
                            m.name.clone(),
                            map_ty(&m.ty),
                            kind,
                            m.is_static,
                            m.is_optional,
                            m.is_readonly,
                        );
                    }
                }
                if let Some(entry) = bind.type_members.interfaces.get(cn) {
                    for m in entry {
                        let kind = map_class_member_kind(m.kind);
                        add_member(
                            &mut results,
                            &mut seen,
                            m.name.clone(),
                            map_ty(&m.ty),
                            kind,
                            m.is_static,
                            m.is_optional,
                            m.is_readonly,
                        );
                    }
                }

                // 2. Check core members
                if let Some(b) = &bind.core {
                    if let Some(entry) = b.class_members.get(cn.as_ref()) {
                        for m in &entry.members {
                            let kind = map_class_member_kind(m.kind);
                            add_member(
                                &mut results,
                                &mut seen,
                                m.name.clone(),
                                map_ty(&m.ty),
                                kind,
                                m.is_static,
                                m.is_optional,
                                m.is_readonly,
                            );
                        }
                    }
                    if let Some(members) = b.flattened_members.get(cn.as_ref()) {
                        for m in members {
                            let kind = map_class_member_kind(m.kind);
                            add_member(
                                &mut results,
                                &mut seen,
                                m.name.clone(),
                                map_ty(&m.ty),
                                kind,
                                m.is_static,
                                m.is_optional,
                                m.is_readonly,
                            );
                        }
                    }
                }

                // 3. Check external / stdlib module binds
                let origin_modules: Vec<String> = origin.iter().map(|s| s.to_string()).collect();
                if let Some(ext_bind) =
                    crate::module_resolver::find_module_bind_for_type_ref(cn, &origin_modules)
                {
                    if let Some(entry) = ext_bind.type_members.classes.get(cn) {
                        for m in &entry.members {
                            let kind = map_class_member_kind(m.kind);
                            add_member(
                                &mut results,
                                &mut seen,
                                m.name.clone(),
                                map_ty(&m.ty),
                                kind,
                                m.is_static,
                                m.is_optional,
                                m.is_readonly,
                            );
                        }
                    }
                }
            }
            TypeKind::Intrinsic(varn_core::TypeTag::Str) => {
                add_member(
                    &mut results,
                    &mut seen,
                    Rc::from("length"),
                    Type::Int,
                    crate::semantic_info::ResolvedMemberKind::Property,
                    false,
                    false,
                    true,
                );
                let str_ty = Type::named("str".to_owned());
                return get_members_of_type(&str_ty, bind);
            }
            TypeKind::Intrinsic(tag) => {
                let named_ty = Type::named(tag.name().to_owned());
                return get_members_of_type(&named_ty, bind);
            }
            _ => {}
        }

        results
}

impl Checker {
    pub(crate) fn collect_member_names(&self, ty: &Type, bind: &BindResult) -> Vec<Rc<str>> {
        match &ty.0 {
            TypeKind::Object(members) => members
                .iter()
                .filter_map(|m| match m {
                    ObjectTypeMember::Property { name, .. }
                    | ObjectTypeMember::Method { name, .. } => Some(name.clone()),
                    _ => None,
                })
                .collect(),
            TypeKind::Named(cn, _) | TypeKind::Generic(cn, _, _) => bind
                .get_class_entry(cn.as_ref())
                .map(|entry| entry.members.iter().map(|m| m.name.clone()).collect())
                .unwrap_or_default(),
            TypeKind::Union(members) => {
                let mut names: Vec<Rc<str>> = Vec::new();
                for m in members {
                    if !m.is_nullable() {
                        names.extend(self.collect_member_names(m, bind));
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
