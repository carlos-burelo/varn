use crate::binder::BindResult;
use crate::checker::Checker;
use crate::types::{ObjectTypeMember, Type};
use std::sync::Arc;
use varn_core::TypeKind;

impl<'r> Checker<'r> {
    pub(crate) fn find_member_info(
        &mut self,
        ty: &Type,
        key: &str,
        bind: &BindResult,
    ) -> Option<(Type, Option<usize>)> {
        let ty_key = (*ty, Arc::from(key));
        if let Some(res) = self.member_type_cache.get(&ty_key) {
            return *res;
        }

        let res = self.find_member_info_uncached(ty, key, bind);
        self.member_type_cache.insert(ty_key, res);
        res
    }

    pub(super) fn find_member_info_uncached(
        &mut self,
        ty: &Type,
        key: &str,
        bind: &BindResult,
    ) -> Option<(Type, Option<usize>)> {
        let ty_kind = self.ty_table.get(ty.0);
        let res = match ty_kind {
            TypeKind::EnumVariant {
                enum_name,
                payload_ty,
                ..
            } => self.member_enum_variant(enum_name, payload_ty, key, bind),
            TypeKind::Named(name_atom, origin_atom) => {
                self.member_named(ty, name_atom, origin_atom, key, bind)
            }
            TypeKind::Generic(name_atom, args_list, origin_atom) => {
                self.member_generic(name_atom, args_list, origin_atom, key, bind)
            }
            TypeKind::Object(mid) => self.member_object(mid, key),
            TypeKind::Union(list) => self.member_union(list, key, bind),
            TypeKind::Array(inner) => self.member_array(inner, key, bind),
            kind @ (TypeKind::Primitive(_) | TypeKind::Builtin(_) | TypeKind::Literal(_)) => {
                match self.member_scalar(&kind, key, bind) {
                    Some(r) => Some(r),
                    None if kind == TypeKind::Builtin(varn_core::BuiltinType::Range) => {
                        self.member_range_fallback(key, bind)
                    }
                    None => None,
                }
            }
            TypeKind::Tuple(_) if key == varn_core::MemberKey::Length.as_str() => {
                Some((Type::Int, None))
            }
            _ => None,
        };
        if res.is_none() {
            if let Some(tn) = crate::checker_expressions::check::members::extension_type_name(
                self,
                ty,
                &self.ty_table,
                bind,
            ) {
                if let Some(mangled) = bind.extensions.methods.get(&tn).and_then(|m| m.get(key)) {
                    let mangled = mangled.clone();
                    if let Some(sym_ty) = super::member_util::extension_method_type(
                        bind,
                        &mangled,
                        &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                    ) {
                        return Some((sym_ty, None));
                    }
                }
                if let Some(mangled) = bind.extensions.getters.get(&tn).and_then(|m| m.get(key)) {
                    if let Some(sym_ty) =
                        super::member_util::extension_getter_type(bind, mangled, &self.ty_table)
                    {
                        return Some((sym_ty, None));
                    }
                }
            }
        }
        res
    }
    pub(crate) fn find_member(
        &mut self,
        ty: &Type,
        key: &str,
        bind: &BindResult,
    ) -> Option<ObjectTypeMember> {
        let ty_kind = self.ty_table.get(ty.0);
        let res = match ty_kind {
            TypeKind::EnumVariant {
                enum_name,
                variant_name: _,
                type_args: _,
                payload_ty,
            } => {
                if key == varn_core::MemberKey::Name.as_str() {
                    return Some(ObjectTypeMember::Property {
                        name: Arc::from(key),
                        ty: Type::Str.0,
                        optional: false,
                        readonly: true,
                    });
                }
                if key == varn_core::MemberKey::Tag.as_str() {
                    return Some(ObjectTypeMember::Property {
                        name: Arc::from(key),
                        ty: Type::Int.0,
                        optional: false,
                        readonly: true,
                    });
                }
                if let Some(res) = self.find_member(&Type::resolved(payload_ty), key, bind) {
                    return Some(res);
                }
                let enum_name_str = self.resolve_bind_atom(bind, enum_name).to_string();
                let named = Type::named(
                    enum_name_str,
                    &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                );
                return self.find_member(&named, key, bind);
            }
            TypeKind::Object(mid) => self
                .ty_table
                .get_object_members(mid)
                .iter()
                .find(|m| m.name() == key)
                .cloned(),
            TypeKind::Named(name_atom, origin_atom) => {
                let name: Arc<str> = self.resolve_bind_atom(bind, name_atom);
                if name.as_ref() == "*" {
                    if let Some(origin_atom) = origin_atom {
                        let origin_path = self.resolve_bind_atom(bind, origin_atom).to_string();
                        let exports = if crate::module_resolver::is_known_module(&origin_path) {
                            Some(self.resolver.stdlib_exports(&origin_path))
                        } else {
                            let mut visiting = Vec::new();
                            Some(self.resolver.module_exports(&origin_path, &mut visiting))
                        };
                        if let Some(exports) = exports {
                            if let Some(sym) = exports.get(key) {
                                let mut sym_ty = sym.ty.unwrap_or(Type::Dynamic);
                                if let Some(origin) = &sym.origin_module {
                                    let origin_str =
                                        self.resolve_bind_atom(bind, *origin).to_string();
                                    let table = std::sync::Arc::make_mut(&mut self.ty_table);
                                    let origin_atom = table.intern_name(&origin_str);
                                    sym_ty = sym_ty.with_origin(origin_atom, table);
                                }
                                return Some(ObjectTypeMember::Property {
                                    name: Arc::from(key),
                                    ty: sym_ty.0,
                                    optional: false,
                                    readonly: true,
                                });
                            }
                        }
                    }
                    return None;
                }
                if let Some(entry) = bind.get_class_entry(name.as_ref()) {
                    if let Some(m) = entry.members.iter().find(|m| m.name.as_ref() == key) {
                        return Some(ObjectTypeMember::Property {
                            name: m.name.clone(),
                            ty: m.ty.0,
                            optional: m.is_optional,
                            readonly: m.is_readonly,
                        });
                    }
                }
                if let Some(parent) = bind.class_parents.get(name.as_ref()) {
                    let parent = parent.clone();
                    let named = self.parent_type(&parent);
                    return self.find_member(&named, key, bind);
                }
                None
            }
            TypeKind::Generic(name_atom, args_list, origin_atom) => {
                let name: Arc<str> = self.resolve_bind_atom(bind, name_atom);
                if let Some(entry) = bind.get_class_entry(name.as_ref()) {
                    if let Some(m) = entry.members.iter().find(|m| m.name.as_ref() == key) {
                        
                        
                        let origin: Option<Arc<str>> =
                            origin_atom.map(|o| self.resolve_bind_atom(bind, o));
                        let args: Vec<Type> = self
                            .ty_table
                            .get_list(args_list)
                            .iter()
                            .map(|id| Type::resolved(*id))
                            .collect();
                        let mapping = super::member_util::generic_mapping(
                            self.resolver,
                            name.as_ref(),
                            &args,
                            origin.as_ref(),
                            bind,
                        );
                        return Some(ObjectTypeMember::Property {
                            name: m.name.clone(),
                            ty: if mapping.is_empty() {
                                m.ty.0
                            } else {
                                m.ty.map_generics(
                                    &mapping,
                                    &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                                )
                                .0
                            },
                            optional: m.is_optional,
                            readonly: m.is_readonly,
                        });
                    }
                }
                if let Some(parent) = bind.class_parents.get(name.as_ref()) {
                    let parent = parent.clone();
                    let named = self.parent_type(&parent);
                    return self.find_member(&named, key, bind);
                }
                None
            }
            _ => None,
        };
        if res.is_none() {
            if let Some(tn) = crate::checker_expressions::check::members::extension_type_name(
                self,
                ty,
                &self.ty_table,
                bind,
            ) {
                if let Some(mangled) = bind.extensions.methods.get(&tn).and_then(|m| m.get(key)) {
                    let mangled = mangled.clone();
                    if let Some(sym) = super::member_util::extension_method_type(
                        bind,
                        &mangled,
                        &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                    ) {
                        let sym_kind = self.ty_table.get(sym.0);
                        let (params, return_type) = match sym_kind {
                            varn_core::TypeKind::Fn(fid) => {
                                let ft = self.ty_table.get_function(fid).clone();
                                (ft.params, ft.return_type)
                            }
                            _ => (vec![], Type::Dynamic.0),
                        };
                        return Some(ObjectTypeMember::Method {
                            name: Arc::from(key),
                            params,
                            return_type,
                            optional: false,
                            is_arrow: false,
                        });
                    }
                }
                if let Some(mangled) = bind.extensions.getters.get(&tn).and_then(|m| m.get(key)) {
                    if let Some(sym) =
                        super::member_util::extension_getter_type(bind, mangled, &self.ty_table)
                    {
                        let has_setter = bind
                            .extensions
                            .setters
                            .get(&tn)
                            .and_then(|m| m.get(key))
                            .is_some();
                        return Some(ObjectTypeMember::Property {
                            name: Arc::from(key),
                            ty: sym.0,
                            optional: false,
                            readonly: !has_setter,
                        });
                    }
                }
            }
        }
        res
    }
}
