use crate::binder::BindResult;
use crate::checker::Checker;
use crate::types::Type;
use std::rc::Rc;
use varn_core::TypeKind;

fn check_in_bind(name: &Rc<str>, key: &str, ext_bind: &crate::binder::BindResult) -> bool {
    if let Some(members) = ext_bind.type_members.classes.get(name) {
        if members.members.iter().any(|m| m.name.as_ref() == key) {
            return true;
        }
    }
    // `type_members.classes` and `get_class_entry` are two views of a class:
    // the latter is what `TypeContext::get_class_members` returns (the one
    // `find_member_info`/`infer_member` consult), and for some binds only it
    // carries every member. Check both, or existence and typing disagree.
    if let Some(entry) = ext_bind.get_class_entry(name) {
        if entry.members.iter().any(|m| m.name.as_ref() == key) {
            return true;
        }
    }
    if let Some(members) = ext_bind.type_members.interfaces.get(name) {
        if members.iter().any(|m| m.name.as_ref() == key) {
            return true;
        }
    }
    if let Some(members) = ext_bind.type_members.namespaces.get(name) {
        if members.iter().any(|m| m.name.as_ref() == key) {
            return true;
        }
    }
    if let Some(members) = ext_bind.get_enum_members_local(name.as_ref()) {
        if members.iter().any(|m| m.name.as_ref() == key) {
            return true;
        }
    }
    if let Some(methods) = ext_bind.get_class_methods_for(name.as_ref()) {
        if methods.contains_key(key) {
            return true;
        }
    }
    false
}

fn check_origin_module(
    resolver: &dyn crate::module_resolver::ImportResolver,
    name: &Rc<str>,
    origin: &Option<Rc<str>>,
    key: &str,
) -> bool {
    let origin_modules: Vec<String> = origin.iter().map(|s| s.to_string()).collect();
    if let Some(ext_bind) = resolver.find_bind_for_type(name, &origin_modules) {
        if check_in_bind(name, key, &ext_bind) {
            return true;
        }
    }

    if origin.is_none() {
        for spec in varn_modules::std_module_ids() {
            if let Some(bind) = resolver.stdlib_bind(spec) {
                if check_in_bind(name, key, &bind) {
                    return true;
                }
            }
        }
    }
    false
}

impl<'r> Checker<'r> {
    pub(crate) fn member_exists_cached(&mut self, ty: &Type, key: &str, bind: &BindResult) -> bool {
        let ty_key = (*ty, Rc::from(key));
        if let Some(exists) = self.member_exists_cache.get(&ty_key) {
            return *exists;
        }

        let exists = self.member_exists(ty, key, bind);
        self.member_exists_cache.insert(ty_key, exists);
        exists
    }

    pub(crate) fn member_exists(&mut self, ty: &Type, key: &str, bind: &BindResult) -> bool {
        let ty_kind = *self.ty_table.get(ty.0);
        let res = match ty_kind {
            TypeKind::Intrinsic(varn_core::TypeTag::Dynamic) => true,
            TypeKind::Intrinsic(varn_core::TypeTag::Never) => false,
            TypeKind::Intrinsic(varn_core::TypeTag::Str) => {
                if key == varn_core::MemberKey::Length.as_str() {
                    return true;
                }
                if let Some(b) = &bind.core {
                    if let Some(members) =
                        b.class_members.get(varn_core::IntrinsicType::Str.as_str())
                    {
                        if members.members.iter().any(|m| m.name.as_ref() == key) {
                            return true;
                        }
                    }
                }
                false
            }
            TypeKind::Intrinsic(varn_core::TypeTag::Bytes) => {
                if key == varn_core::MemberKey::Length.as_str() {
                    return true;
                }
                if let Some(b) = &bind.core {
                    if let Some(members) =
                        b.class_members.get(varn_core::IntrinsicType::Bytes.as_str())
                    {
                        if members.members.iter().any(|m| m.name.as_ref() == key) {
                            return true;
                        }
                    }
                }
                false
            }
            TypeKind::Intrinsic(_) => {
                let name = ty.display(&self.ty_table, &bind.interner).to_string();
                if let Some(b) = &bind.core {
                    if let Some(members) = b.class_members.get(name.as_str()) {
                        if members.members.iter().any(|m| m.name.as_ref() == key) {
                            return true;
                        }
                    }
                }
                false
            }
            TypeKind::EnumVariant {
                enum_name,
                variant_name: _,
                type_args: _,
                payload_ty,
            } => {
                if key == varn_core::MemberKey::RawValue.as_str()
                    || key == varn_core::MemberKey::Tag.as_str()
                    || key == varn_core::MemberKey::Name.as_str()
                    || key == varn_core::MemberKey::VariantName.as_str()
                {
                    return true;
                }
                if self.member_exists(&Type(payload_ty, false), key, bind) {
                    return true;
                }
                let enum_name_str = self.resolve_bind_atom(bind, enum_name).to_string();
                let named = Type::named(enum_name_str, self.resolver, &mut self.ty_table);
                self.member_exists(&named, key, bind)
            }
            TypeKind::Named(name_atom, origin_atom) => {
                let name: Rc<str> = self.resolve_bind_atom(bind, name_atom);
                let origin: Option<Rc<str>> =
                    origin_atom.map(|o| self.resolve_bind_atom(bind, o));
                if name.as_ref() == "*" {
                    if let Some(origin_path) = &origin {
                        let exports = if crate::module_resolver::is_known_module(origin_path) {
                            Some(self.resolver.stdlib_exports(origin_path))
                        } else {
                            let mut visiting = Vec::new();
                            Some(self.resolver.module_exports(origin_path, &mut visiting))
                        };
                        if let Some(exports) = exports {
                            if exports.contains_key(key) {
                                return true;
                            }
                        }
                    }
                }

                if name.as_ref() == varn_core::IntrinsicType::Str.as_str()
                    && key == varn_core::MemberKey::Length.as_str()
                {
                    return true;
                }

                let origin_modules: Vec<String> = origin.iter().map(|s| s.to_string()).collect();
                let is_enum = super::is_enum_type(self.resolver, bind, &name, &origin_modules);

                if is_enum {
                    if key == varn_core::MemberKey::RawValue.as_str()
                        || key == varn_core::MemberKey::Tag.as_str()
                        || key == varn_core::MemberKey::Name.as_str()
                        || key == varn_core::MemberKey::VariantName.as_str()
                    {
                        return true;
                    }

                    let mut variants = Vec::new();
                    if let Some(members) = bind.get_enum_members_local(name.as_ref()) {
                        variants.extend(members.iter().map(|m| m.name.clone()));
                    }
                    if let Some(ext_bind) = self.resolver.find_bind_for_type(&name, &origin_modules)
                    {
                        if let Some(members) = ext_bind.get_enum_members_local(name.as_ref()) {
                            variants.extend(members.iter().map(|m| m.name.clone()));
                        }
                    }
                    if variants.iter().any(|v| v.as_ref() == key) {
                        return true;
                    }
                    for v in &variants {
                        if let Some(fields) = bind.sum_variant_fields.get(v) {
                            if fields.iter().any(|(fname, _)| fname.as_ref() == key) {
                                return true;
                            }
                        }
                        if let Some(ext_bind) =
                            self.resolver.find_bind_for_type(&name, &origin_modules)
                        {
                            if let Some(fields) = ext_bind.sum_variant_fields.get(v) {
                                if fields.iter().any(|(fname, _)| fname.as_ref() == key) {
                                    return true;
                                }
                            }
                        }
                    }
                }

                if let Some(members) = bind.type_members.classes.get(&name) {
                    if members.members.iter().any(|m| m.name.as_ref() == key) {
                        return true;
                    }
                }
                if let Some(members) = bind.type_members.interfaces.get(&name) {
                    if members.iter().any(|m| m.name.as_ref() == key) {
                        return true;
                    }
                }
                if let Some(members) = bind.type_members.enums.get(&name) {
                    if members.iter().any(|m| m.name.as_ref() == key) {
                        return true;
                    }
                }
                if let Some(members) = bind.type_members.namespaces.get(&name) {
                    if members.iter().any(|m| m.name.as_ref() == key) {
                        return true;
                    }
                }

                if bind
                    .get_class_methods_for(name.as_ref())
                    .is_some_and(|m| m.contains_key(key))
                {
                    return true;
                }
                if let Some(b) = &bind.core {
                    if let Some(members) = b.class_members.get(name.as_ref()) {
                        if members.members.iter().any(|m| m.name.as_ref() == key) {
                            return true;
                        }
                    }
                    if let Some(members) = b.interface_members.get(name.as_ref()) {
                        if members.iter().any(|m| m.name.as_ref() == key) {
                            return true;
                        }
                    }
                }
                if let Some(parent) = bind.class_parents.get(&name) {
                    let parent = parent.clone();
                    let named = Type::named(parent, self.resolver, &mut self.ty_table);
                    return self.member_exists(&named, key, bind);
                }

                if check_origin_module(self.resolver, &name, &origin, key) {
                    return true;
                }
                false
            }
            TypeKind::Generic(name_atom, _, origin_atom) => {
                let name = self.resolve_bind_atom(bind, name_atom).to_string();
                let origin: Option<Rc<str>> =
                    origin_atom.map(|o| self.resolve_bind_atom(bind, o));
                let ty = Type::named_with_origin(name, origin, self.resolver, &mut self.ty_table);
                self.member_exists(&ty, key, bind)
            }
            TypeKind::Object(mid) => self
                .ty_table
                .get_object_members(mid)
                .iter()
                .any(|m| m.name() == key),
            // Mirrors the `TypeKind::Tuple` arm of `find_member_info_uncached`.
            //
            // Having to say it twice is the defect, not the answer: "which
            // members does this type have" is walked once here for existence
            // and once there for the type, and the two can disagree — as they
            // did, which is why adding it in one place left the diagnostic
            // still firing. Collapsing `member_exists` into
            // `find_member_info(..).is_some()` is the real fix; it is a change
            // across a 15k-line crate and does not belong in this one.
            TypeKind::Tuple(_) => key == varn_core::MemberKey::Length.as_str(),
            TypeKind::Array(_) => {
                if let Some(b) = &bind.core {
                    if let Some(members) = b
                        .class_members
                        .get(varn_core::IntrinsicType::Array.as_str())
                    {
                        return members.members.iter().any(|m| m.name.as_ref() == key);
                    }
                }
                false
            }
            TypeKind::Union(list) => {
                let ids = self.ty_table.get_list(list).to_vec();
                ids.iter().all(|id| self.member_exists(&Type(*id, false), key, bind))
            }
            TypeKind::Intersection(list) => {
                let ids = self.ty_table.get_list(list).to_vec();
                ids.iter().any(|id| self.member_exists(&Type(*id, false), key, bind))
            }
            _ => false,
        };
        if !res {
            if let Some(tn) = crate::checker_expressions::check::members::extension_type_name(
                self,
                ty,
                &self.ty_table,
                bind,
            ) {
                if bind
                    .extensions
                    .methods
                    .get(&tn)
                    .is_some_and(|m| m.contains_key(key))
                {
                    return true;
                }
                if bind
                    .extensions
                    .getters
                    .get(&tn)
                    .is_some_and(|m| m.contains_key(key))
                {
                    return true;
                }
                if bind
                    .extensions
                    .setters
                    .get(&tn)
                    .is_some_and(|m| m.contains_key(key))
                {
                    return true;
                }
            }
        }
        res
    }
}
