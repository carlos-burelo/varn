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
    if let Some(methods) = ext_bind.get_class_methods_for(name.as_ref()) {
        if methods.contains_key(key) {
            return true;
        }
    }
    false
}

fn check_origin_module(name: &Rc<str>, origin: &Option<Rc<str>>, key: &str) -> bool {
    let origin_modules: Vec<String> = origin.iter().map(|s| s.to_string()).collect();
    if let Some(ext_bind) = crate::module_resolver::find_module_bind_for_type_ref(name, &origin_modules)
    {
        if check_in_bind(name, key, &ext_bind) {
            return true;
        }
    }

    if origin.is_none() {
        for spec in varn_modules::STD_MODULES {
            if let Some(bind) = crate::module_resolver::resolve_stdlib_module_bind_ref(spec) {
                if check_in_bind(name, key, &bind) {
                    return true;
                }
            }
        }
    }
    false
}

impl Checker {
    pub(crate) fn member_exists_cached(&mut self, ty: &Type, key: &str, bind: &BindResult) -> bool {
        let ty_key = (ty.clone(), Rc::from(key));
        if let Some(exists) = self.member_exists_cache.get(&ty_key) {
            return *exists;
        }

        let exists = self.member_exists(ty, key, bind);
        self.member_exists_cache.insert(ty_key, exists);
        exists
    }

    pub(crate) fn member_exists(&self, ty: &Type, key: &str, bind: &BindResult) -> bool {
        let res = match &ty.0 {
            TypeKind::Intrinsic(varn_core::TypeTag::Dynamic) => true,
            TypeKind::Intrinsic(varn_core::TypeTag::Never) => false,
            TypeKind::Intrinsic(varn_core::TypeTag::Str) => {
                if key == "length" {
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
            TypeKind::Intrinsic(_) => {
                let name = ty.to_string();
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
                if key == "rawValue" || key == "__tag" || key == "name" || key == "__variant_name__"
                {
                    return true;
                }
                if self.member_exists(payload_ty, key, bind) {
                    return true;
                }
                self.member_exists(&Type::named(enum_name.clone()), key, bind)
            }
            TypeKind::Named(name, origin) => {
                if name.as_ref() == varn_core::IntrinsicType::Str.as_str() && key == "length" {
                    return true;
                }

                let origin_modules: Vec<String> = origin.iter().map(|s| s.to_string()).collect();
                let is_enum = bind.get_enum_members_local(name.as_ref()).is_some()
                    || bind
                        .core
                        .as_ref()
                        .map_or(false, |b| b.enum_members.contains_key(name.as_ref()))
                    || crate::module_resolver::find_module_bind_for_type_ref(name, &origin_modules)
                        .as_ref()
                        .map_or(false, |eb| {
                            eb.get_enum_members_local(name.as_ref()).is_some()
                        });

                if is_enum {
                    if key == "rawValue"
                        || key == "__tag"
                        || key == "name"
                        || key == "__variant_name__"
                    {
                        return true;
                    }

                    let mut variants = Vec::new();
                    if let Some(members) = bind.get_enum_members_local(name.as_ref()) {
                        variants.extend(members.iter().map(|m| m.name.clone()));
                    }
                    if let Some(ext_bind) =
                        crate::module_resolver::find_module_bind_for_type_ref(name, &origin_modules)
                    {
                        if let Some(members) = ext_bind.get_enum_members_local(name.as_ref()) {
                            variants.extend(members.iter().map(|m| m.name.clone()));
                        }
                    }
                    for v in &variants {
                        if let Some(fields) = bind.sum_variant_fields.get(v) {
                            if fields.iter().any(|(fname, _)| fname.as_ref() == key) {
                                return true;
                            }
                        }
                        if let Some(ext_bind) =
                            crate::module_resolver::find_module_bind_for_type_ref(name, &origin_modules)
                        {
                            if let Some(fields) = ext_bind.sum_variant_fields.get(v) {
                                if fields.iter().any(|(fname, _)| fname.as_ref() == key) {
                                    return true;
                                }
                            }
                        }
                    }
                }

                if let Some(members) = bind.type_members.classes.get(name) {
                    if members.members.iter().any(|m| m.name.as_ref() == key) {
                        return true;
                    }
                }
                if let Some(members) = bind.type_members.interfaces.get(name) {
                    if members.iter().any(|m| m.name.as_ref() == key) {
                        return true;
                    }
                }
                if let Some(members) = bind.type_members.enums.get(name) {
                    if members.iter().any(|m| m.name.as_ref() == key) {
                        return true;
                    }
                }
                if let Some(members) = bind.type_members.namespaces.get(name) {
                    if members.iter().any(|m| m.name.as_ref() == key) {
                        return true;
                    }
                }

                if bind
                    .get_class_methods_for(name.as_ref())
                    .map_or(false, |m| m.contains_key(key))
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
                if let Some(parent) = bind.class_parents.get(name) {
                    return self.member_exists(&Type::named(parent.clone()), key, bind);
                }

                if check_origin_module(name, origin, key) {
                    return true;
                }
                false
            }
            TypeKind::Generic(name, _, origin) => {
                let ty = Type(TypeKind::Named(name.clone(), origin.clone()), false);
                self.member_exists(&ty, key, bind)
            }
            TypeKind::Object(members) => members.iter().any(|m| m.name() == key),
            TypeKind::Array(_) => {
                if let Some(b) = &bind.core {
                    if let Some(members) = b.class_members.get("Array") {
                        return members.members.iter().any(|m| m.name.as_ref() == key);
                    }
                }
                false
            }
            TypeKind::Union(members) => members.iter().all(|m| self.member_exists(m, key, bind)),
            TypeKind::Intersection(members) => {
                members.iter().any(|m| self.member_exists(m, key, bind))
            }
            TypeKind::LiteralStr(_) => self.member_exists(&Type::Str, key, bind),
            TypeKind::LiteralInt(_) => self.member_exists(&Type::Int, key, bind),
            TypeKind::LiteralFloat(_) => self.member_exists(&Type::Float, key, bind),
            TypeKind::LiteralBool(_) => self.member_exists(&Type::Bool, key, bind),
            _ => false,
        };
        if !res {
            if let Some(tn) = crate::checker_expressions::check::members::extension_type_name(ty) {
                if bind
                    .extensions
                    .methods
                    .get(&tn)
                    .map_or(false, |m| m.contains_key(key))
                {
                    return true;
                }
                if bind
                    .extensions
                    .getters
                    .get(&tn)
                    .map_or(false, |m| m.contains_key(key))
                {
                    return true;
                }
                if bind
                    .extensions
                    .setters
                    .get(&tn)
                    .map_or(false, |m| m.contains_key(key))
                {
                    return true;
                }
            }
        }
        res
    }
}
