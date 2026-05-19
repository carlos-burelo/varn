use crate::binder::BindResult;
use crate::checker::Checker;
use crate::types::{ObjectTypeMember, Type};
use std::rc::Rc;
use varn_core::TypeKind;

impl Checker {
    pub(crate) fn find_member_info(
        &mut self,
        ty: &Type,
        key: &str,
        bind: &BindResult,
    ) -> Option<(Type, Option<usize>)> {
        let ty_key = (ty.clone(), Rc::from(key));
        if let Some(res) = self.member_type_cache.get(&ty_key) {
            return res.clone();
        }

        let res = self.find_member_info_uncached(ty, key, bind);
        self.member_type_cache.insert(ty_key, res.clone());
        res
    }

    fn find_member_info_uncached(
        &self,
        ty: &Type,
        key: &str,
        bind: &BindResult,
    ) -> Option<(Type, Option<usize>)> {
        let res = match &ty.0 {
            TypeKind::Named(name, origin) => {
                if name.as_ref() == varn_core::IntrinsicType::Str.as_str() && key == "length" {
                    return Some((Type::Int, None));
                }
                if let Some(members) = bind.type_members.classes.get(name) {
                    if let Some(m) = members.members.iter().find(|m| m.name.as_ref() == key) {
                        return Some((m.ty.clone(), m.symbol_id));
                    }
                }
                if let Some(members) = bind.type_members.interfaces.get(name) {
                    if let Some(m) = members.iter().find(|m| m.name.as_ref() == key) {
                        return Some((m.ty.clone(), m.symbol_id));
                    }
                }

                if let Some(ty) = bind
                    .get_class_methods_for(name.as_ref())
                    .and_then(|m| m.get(key))
                {
                    return Some((ty.clone(), None));
                }
                if let Some(b) = &bind.builtin {
                    if let Some(members) = b.class_members.get(name.as_ref()) {
                        if let Some(m) = members.members.iter().find(|m| m.name.as_ref() == key) {
                            return Some((m.ty.clone(), m.symbol_id));
                        }
                    }
                }
                if let Some(parent) = bind.class_parents.get(name) {
                    return self.find_member_info_uncached(&Type::named(parent.clone()), key, bind);
                }

                let origin_modules: Vec<String> = origin.iter().map(|s| s.to_string()).collect();
                let ext_bind_opt =
                    crate::module_resolver::find_module_bind_for_type(name, &origin_modules);
                let candidates: Box<dyn Iterator<Item = crate::binder::BindResult>> =
                    if let Some(b) = ext_bind_opt {
                        Box::new(std::iter::once(b))
                    } else if origin.is_none() {
                        Box::new(varn_modules::STD_MODULES.iter().filter_map(|spec| {
                            crate::module_resolver::resolve_stdlib_module_bind(spec)
                        }))
                    } else {
                        Box::new(std::iter::empty())
                    };
                for ext_bind in candidates {
                    if let Some(members) = ext_bind.type_members.classes.get(name) {
                        if let Some(m) = members.members.iter().find(|m| m.name.as_ref() == key) {
                            return Some((m.ty.clone(), m.symbol_id));
                        }
                    }
                    if let Some(members) = ext_bind.type_members.interfaces.get(name) {
                        if let Some(m) = members.iter().find(|m| m.name.as_ref() == key) {
                            return Some((m.ty.clone(), m.symbol_id));
                        }
                    }
                    if let Some(ty) = ext_bind
                        .get_class_methods_for(name.as_ref())
                        .and_then(|m| m.get(key))
                    {
                        return Some((ty.clone(), None));
                    }
                }
                None
            }
            TypeKind::Generic(name, _, origin) => {
                let ty = crate::types::Type(TypeKind::Named(name.clone(), origin.clone()), false);
                self.find_member_info_uncached(&ty, key, bind)
            }
            TypeKind::Object(members) => members.iter().find(|m| m.name() == key).map(|m| {
                let ty = match m {
                    ObjectTypeMember::Property { ty, .. } => ty.clone(),
                    ObjectTypeMember::Method {
                        params,
                        return_type,

                        is_arrow,
                        ..
                    } => Type::fn_(crate::types::FunctionType {
                        params: params.clone(),
                        return_type: return_type.clone(),
                        is_arrow: *is_arrow,
                        type_params: vec![],
                    }),
                    _ => Type::Dynamic,
                };
                (ty, None)
            }),
            TypeKind::Union(members) => {
                let infos: Vec<(Type, Option<usize>)> = members
                    .iter()
                    .filter_map(|m| self.find_member_info_uncached(m, key, bind))
                    .collect();
                if infos.len() == members.len() {
                    let first_sid = infos[0].1;
                    let all_same_sid = infos.iter().all(|i| i.1 == first_sid);
                    let types: Vec<Type> = infos.into_iter().map(|i| i.0).collect();

                    let collapsed = if types.windows(2).all(|w| w[0] == w[1]) {
                        types.into_iter().next().unwrap()
                    } else {
                        Type::union(types)
                    };
                    Some((collapsed, if all_same_sid { first_sid } else { None }))
                } else {
                    None
                }
            }
            TypeKind::LiteralStr(_) => self.find_member_info_uncached(&Type::Str, key, bind),
            TypeKind::LiteralInt(_) => self.find_member_info_uncached(&Type::Int, key, bind),
            TypeKind::LiteralFloat(_) => self.find_member_info_uncached(&Type::Float, key, bind),
            TypeKind::LiteralBool(_) => self.find_member_info_uncached(&Type::Bool, key, bind),
            TypeKind::Intrinsic(varn_core::TypeTag::Str) => {
                if key == "length" {
                    Some((Type::Int, None))
                } else {
                    bind.builtin
                        .as_ref()
                        .and_then(|b| b.class_members.get(varn_core::IntrinsicType::Str.as_str()))
                        .and_then(|members| {
                            members
                                .members
                                .iter()
                                .find(|m| m.name.as_ref() == key)
                                .map(|m| (m.ty.clone(), m.symbol_id))
                        })
                }
            }
            _ => None,
        };
        if res.is_none() {
            if let Some(tn) = crate::checker_expressions::check::members::extension_type_name(ty) {
                if let Some(mangled) = bind.extensions.methods.get(&tn).and_then(|m| m.get(key)) {
                    if let Some(sym) = bind
                        .class_methods
                        .get(&tn)
                        .and_then(|m| m.get(mangled.as_ref()))
                    {
                        return Some((sym.clone(), None));
                    }
                }
                if let Some(mangled) = bind.extensions.getters.get(&tn).and_then(|m| m.get(key)) {
                    if let Some(sym) = bind
                        .class_methods
                        .get(&tn)
                        .and_then(|m| m.get(mangled.as_ref()))
                    {
                        return Some((sym.clone(), None));
                    }
                }
            }
        }
        res
    }

    pub(crate) fn find_member(
        &self,
        ty: &Type,
        key: &str,
        bind: &BindResult,
    ) -> Option<ObjectTypeMember> {
        let res = match &ty.0 {
            TypeKind::Object(members) => members.iter().find(|m| m.name() == key).cloned(),
            TypeKind::Named(name, _) | TypeKind::Generic(name, _, _) => {
                if let Some(entry) = bind.get_class_entry(name.as_ref()) {
                    if let Some(m) = entry.members.iter().find(|m| m.name.as_ref() == key) {
                        return Some(ObjectTypeMember::Property {
                            name: m.name.clone(),
                            ty: m.ty.clone(),
                            optional: m.is_optional,
                            readonly: m.is_readonly,
                        });
                    }
                }
                if let Some(parent) = bind.class_parents.get(name.as_ref()) {
                    return self.find_member(&Type::named(parent.clone()), key, bind);
                }
                None
            }
            _ => None,
        };
        if res.is_none() {
            if let Some(tn) = crate::checker_expressions::check::members::extension_type_name(ty) {
                if let Some(mangled) = bind.extensions.methods.get(&tn).and_then(|m| m.get(key)) {
                    if let Some(sym) = bind
                        .class_methods
                        .get(&tn)
                        .and_then(|m| m.get(mangled.as_ref()))
                    {
                        return Some(ObjectTypeMember::Method {
                            name: Rc::from(key),
                            params: match &sym.0 {
                                varn_core::TypeKind::Fn(ft) => ft.params.clone(),
                                _ => vec![],
                            },
                            return_type: Box::new(match &sym.0 {
                                varn_core::TypeKind::Fn(ft) => *ft.return_type.clone(),
                                _ => Type::Dynamic,
                            }),
                            optional: false,
                            is_arrow: false,
                        });
                    }
                }
                if let Some(mangled) = bind.extensions.getters.get(&tn).and_then(|m| m.get(key)) {
                    if let Some(sym) = bind
                        .class_methods
                        .get(&tn)
                        .and_then(|m| m.get(mangled.as_ref()))
                    {
                        return Some(ObjectTypeMember::Property {
                            name: Rc::from(key),
                            ty: sym.clone(),
                            optional: false,
                            readonly: true,
                        });
                    }
                }
            }
        }
        res
    }
}
