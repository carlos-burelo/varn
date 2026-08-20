use crate::binder::BindResult;
use crate::checker::Checker;
use crate::types::{ObjectTypeMember, Type};
use rustc_hash::FxHashMap;
use std::rc::Rc;
use varn_core::TypeKind;

/// Declared type-parameter names of a class/interface, by name. Mirrors
/// `Checker::symbol_type_params_any` without its cache, which needs `&mut`,
/// and additionally follows `origin` into the declaring module: a generic type
/// can flow in as a call's return type (`channel<int>()` -> `Channel<int>`)
/// without `Channel` itself ever being imported here.
fn class_type_params(name: &str, origin: Option<&Rc<str>>, bind: &BindResult) -> Vec<Rc<str>> {
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
    if let Some(b) = crate::module_resolver::find_module_bind_for_type_ref(name, &origins) {
        return params_in(name, &b);
    }
    if origin.is_none() {
        for spec in varn_modules::std_module_ids() {
            if let Some(b) = crate::module_resolver::resolve_stdlib_module_bind_ref(spec) {
                let params = params_in(name, &b);
                if !params.is_empty() {
                    return params;
                }
            }
        }
    }
    Vec::new()
}

fn params_in(name: &str, bind: &BindResult) -> Vec<Rc<str>> {
    bind.scopes
        .get(bind.global_scope)
        .resolve(name, &bind.scopes)
        .map(|sid| bind.arena.get(sid).type_params.clone())
        .unwrap_or_default()
}

/// `{ T -> int }` for a `Generic("Channel", [int])` receiver. Empty when the
/// class declares no parameters or the receiver carries no arguments.
pub(crate) fn generic_mapping(
    name: &str,
    args: &[Type],
    origin: Option<&Rc<str>>,
    bind: &BindResult,
) -> FxHashMap<Rc<str>, Type> {
    if args.is_empty() {
        return FxHashMap::default();
    }
    class_type_params(name, origin, bind)
        .into_iter()
        .zip(args.iter().cloned())
        .collect()
}

fn resolve_extension_symbol_type(bind: &BindResult, mangled: &Rc<str>) -> Option<Type> {
    let scope = bind.scopes.get(bind.global_scope);
    let sid = scope.resolve(mangled.as_ref(), &bind.scopes)?;
    bind.arena.get(sid).ty.clone()
}

fn extension_method_type(bind: &BindResult, mangled: &Rc<str>) -> Option<Type> {
    let sym_ty = resolve_extension_symbol_type(bind, mangled)?;
    let TypeKind::Fn(ft) = &sym_ty.0 else {
        return Some(sym_ty);
    };

    let params = ft
        .params
        .iter()
        .skip_while(|p| p.name.as_deref() == Some("this"))
        .cloned()
        .collect();

    Some(Type::fn_(crate::types::FunctionType {
        params,
        return_type: ft.return_type.clone(),
        is_arrow: ft.is_arrow,
        type_params: ft.type_params.clone(),
    }))
}

fn extension_getter_type(bind: &BindResult, mangled: &Rc<str>) -> Option<Type> {
    let sym_ty = resolve_extension_symbol_type(bind, mangled)?;
    match &sym_ty.0 {
        TypeKind::Fn(ft) => Some(ft.return_type.as_ref().clone()),
        _ => Some(sym_ty),
    }
}

fn intrinsic_member_info(
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
                .map(|m| (m.ty.clone(), m.symbol_id))
        })
}

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
            TypeKind::EnumVariant {
                enum_name,
                variant_name: _,
                type_args: _,
                payload_ty,
            } => {
                if key == "name" || key == "__variant_name__" {
                    return Some((Type::Str, None));
                }
                if key == "__tag" || key == "rawValue" {
                    return Some((Type::Int, None));
                }
                if let Some(res) = self.find_member_info_uncached(payload_ty, key, bind) {
                    return Some(res);
                }
                return self.find_member_info_uncached(&Type::named(enum_name.clone()), key, bind);
            }
            TypeKind::Named(name, origin) => {
                if name.as_ref() == "*" {
                    if let Some(origin_path) = origin {
                        let exports = if crate::module_resolver::is_known_module(origin_path) {
                            Some(crate::module_resolver::resolve_stdlib_module_exports_ref(
                                origin_path,
                            ))
                        } else {
                            let mut visiting = Vec::new();
                            Some(crate::module_resolver::resolve_module_exports_ref(
                                origin_path,
                                &mut visiting,
                            ))
                        };
                        if let Some(exports) = exports {
                            if let Some(sym) = exports.get(key) {
                                let mut sym_ty = sym.ty.clone().unwrap_or(Type::Dynamic);
                                if let Some(origin) = &sym.origin_module {
                                    sym_ty = sym_ty.with_origin(origin.clone());
                                }
                                return Some((sym_ty, None));
                            }
                        }
                    }
                    return None;
                }

                if name.as_ref() == varn_core::IntrinsicType::Str.as_str() && key == "length" {
                    return Some((Type::Int, None));
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
                    if key == "rawValue" || key == "__tag" {
                        return Some((Type::Int, None));
                    }
                    if key == "name" || key == "__variant_name__" {
                        return Some((Type::Str, None));
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
                    let mut found_tys = Vec::new();
                    for v in &variants {
                        if let Some(fields) = bind.sum_variant_fields.get(v) {
                            if let Some((_, ty)) =
                                fields.iter().find(|(fname, _)| fname.as_ref() == key)
                            {
                                found_tys.push(ty.clone());
                            }
                        }
                        if let Some(ext_bind) =
                            crate::module_resolver::find_module_bind_for_type_ref(
                                name,
                                &origin_modules,
                            )
                        {
                            if let Some(fields) = ext_bind.sum_variant_fields.get(v) {
                                if let Some((_, ty)) =
                                    fields.iter().find(|(fname, _)| fname.as_ref() == key)
                                {
                                    found_tys.push(ty.clone());
                                }
                            }
                        }
                    }
                    if !found_tys.is_empty() {
                        return Some((Type::union(found_tys), None));
                    }
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
                if let Some(members) = bind.get_enum_members_local(name.as_ref()) {
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
                if let Some(b) = &bind.core {
                    if let Some(members) = b.class_members.get(name.as_ref()) {
                        if let Some(m) = members.members.iter().find(|m| m.name.as_ref() == key) {
                            return Some((m.ty.clone(), m.symbol_id));
                        }
                    }
                    if let Some(members) = b.flattened_members.get(name.as_ref()) {
                        if let Some(m) = members.iter().find(|m| m.name.as_ref() == key) {
                            return Some((m.ty.clone(), m.symbol_id));
                        }
                    }
                    if let Some(methods) = b.class_methods.get(name.as_ref()) {
                        if let Some(ty) = methods.get(key) {
                            return Some((ty.clone(), None));
                        }
                    }
                }
                if let Some(parent) = bind.class_parents.get(name) {
                    return self.find_member_info_uncached(&Type::named(parent.clone()), key, bind);
                }

                let origin_modules: Vec<String> = origin.iter().map(|s| s.to_string()).collect();
                let ext_bind_opt =
                    crate::module_resolver::find_module_bind_for_type_ref(name, &origin_modules);
                let candidates: Box<dyn Iterator<Item = Rc<crate::binder::BindResult>>> =
                    if let Some(b) = ext_bind_opt {
                        Box::new(std::iter::once(b))
                    } else if origin.is_none() {
                        Box::new(
                            varn_modules::std_module_ids()
                                .into_iter()
                                .filter_map(|spec| {
                                    crate::module_resolver::resolve_stdlib_module_bind_ref(spec)
                                }),
                        )
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
                    if let Some(members) = ext_bind.get_enum_members_local(name.as_ref()) {
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
            TypeKind::Generic(name, args, origin) => {
                let base = crate::types::Type(TypeKind::Named(name.clone(), origin.clone()), false);
                // The member is declared against the class's type PARAMETERS
                // (`Channel<T>.rx: Receiver<T>`); the receiver carries the
                // ARGUMENTS. Without this substitution `channel<int>(…).rx`
                // reads as `Receiver<T>` and every use of the element type sees
                // an unbound `T`.
                self.find_member_info_uncached(&base, key, bind)
                    .map(|(member_ty, sym)| {
                        let mapping = generic_mapping(name.as_ref(), args, origin.as_ref(), bind);
                        if mapping.is_empty() {
                            (member_ty, sym)
                        } else {
                            (member_ty.map_generics(&mapping), sym)
                        }
                    })
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
            TypeKind::Array(inner) => {
                let array_ty = Type::generic(
                    varn_core::IntrinsicType::Array.as_str().to_owned(),
                    vec![*inner.clone()],
                );
                self.find_member_info_uncached(&array_ty, key, bind)
            }
            TypeKind::Intrinsic(varn_core::TypeTag::Str) => {
                if key == "length" {
                    Some((Type::Int, None))
                } else {
                    intrinsic_member_info(bind, varn_core::IntrinsicType::Str.as_str(), key)
                }
            }
            // A tuple's length is known at check time — it is the arity of the
            // type itself. The runtime has always answered `.length` on a
            // tuple (the heap stores it as an array; `is_array` accepts
            // `Tuple`), but the checker had no member table for tuples at all,
            // so `#[1, "a", true].length` was rejected in any file the checker
            // actually looked at. It went unnoticed because imported modules
            // had their diagnostics discarded, and `tests/69-tuples-records.vn`
            // is only ever reached through an import.
            TypeKind::Tuple(_) if key == "length" => Some((Type::Int, None)),
            TypeKind::Intrinsic(tag) => {
                if let Some(info) = intrinsic_member_info(bind, tag.name(), key) {
                    Some(info)
                } else if *tag == varn_core::TypeTag::Range {
                    let range_ty = Type::named(varn_core::IntrinsicType::Range.as_str().to_owned());
                    self.find_member_info_uncached(&range_ty, key, bind)
                } else {
                    None
                }
            }
            _ => None,
        };
        if res.is_none() {
            if let Some(tn) = crate::checker_expressions::check::members::extension_type_name(ty) {
                if let Some(mangled) = bind.extensions.methods.get(&tn).and_then(|m| m.get(key)) {
                    if let Some(sym_ty) = extension_method_type(bind, mangled) {
                        return Some((sym_ty, None));
                    }
                }
                if let Some(mangled) = bind.extensions.getters.get(&tn).and_then(|m| m.get(key)) {
                    if let Some(sym_ty) = extension_getter_type(bind, mangled) {
                        return Some((sym_ty, None));
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
            TypeKind::EnumVariant {
                enum_name,
                variant_name: _,
                type_args: _,
                payload_ty,
            } => {
                if key == "name" {
                    return Some(ObjectTypeMember::Property {
                        name: Rc::from(key),
                        ty: Type::Str,
                        optional: false,
                        readonly: true,
                    });
                }
                if key == "__tag" {
                    return Some(ObjectTypeMember::Property {
                        name: Rc::from(key),
                        ty: Type::Int,
                        optional: false,
                        readonly: true,
                    });
                }
                if let Some(res) = self.find_member(payload_ty, key, bind) {
                    return Some(res);
                }
                return self.find_member(&Type::named(enum_name.clone()), key, bind);
            }
            TypeKind::Object(members) => members.iter().find(|m| m.name() == key).cloned(),
            TypeKind::Named(name, origin) => {
                if name.as_ref() == "*" {
                    if let Some(origin_path) = origin {
                        let exports = if crate::module_resolver::is_known_module(origin_path) {
                            Some(crate::module_resolver::resolve_stdlib_module_exports_ref(
                                origin_path,
                            ))
                        } else {
                            let mut visiting = Vec::new();
                            Some(crate::module_resolver::resolve_module_exports_ref(
                                origin_path,
                                &mut visiting,
                            ))
                        };
                        if let Some(exports) = exports {
                            if let Some(sym) = exports.get(key) {
                                let mut sym_ty = sym.ty.clone().unwrap_or(Type::Dynamic);
                                if let Some(origin) = &sym.origin_module {
                                    sym_ty = sym_ty.with_origin(origin.clone());
                                }
                                return Some(ObjectTypeMember::Property {
                                    name: Rc::from(key),
                                    ty: sym_ty,
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
            TypeKind::Generic(name, args, origin) => {
                if let Some(entry) = bind.get_class_entry(name.as_ref()) {
                    if let Some(m) = entry.members.iter().find(|m| m.name.as_ref() == key) {
                        // Same substitution as `find_member_info_uncached`: the
                        // member's type is written in the class's parameters.
                        let mapping = generic_mapping(name.as_ref(), args, origin.as_ref(), bind);
                        return Some(ObjectTypeMember::Property {
                            name: m.name.clone(),
                            ty: if mapping.is_empty() {
                                m.ty.clone()
                            } else {
                                m.ty.map_generics(&mapping)
                            },
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
                    if let Some(sym) = extension_method_type(bind, mangled) {
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
                    if let Some(sym) = extension_getter_type(bind, mangled) {
                        let has_setter = bind
                            .extensions
                            .setters
                            .get(&tn)
                            .and_then(|m| m.get(key))
                            .is_some();
                        return Some(ObjectTypeMember::Property {
                            name: Rc::from(key),
                            ty: sym.clone(),
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
