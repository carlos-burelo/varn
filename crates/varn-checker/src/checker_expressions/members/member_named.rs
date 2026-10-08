use super::Checker;
use std::sync::Arc;
use varn_sem::bind::BindResult;
use varn_sem::types::Type;

impl<'r> Checker<'r> {
    pub(super) fn member_enum_variant(
        &mut self,
        enum_name: varn_core::Atom,
        payload_ty: varn_sem::types::CheckerTyId,
        key: &str,
        bind: &BindResult,
    ) -> Option<(Type, Option<usize>)> {
        if key == varn_core::MemberKey::Name.as_str()
            || key == varn_core::MemberKey::VariantName.as_str()
        {
            return Some((Type::Str, None));
        }
        if key == varn_core::MemberKey::Tag.as_str()
            || key == varn_core::MemberKey::RawValue.as_str()
        {
            return Some((Type::Int, None));
        }
        if let Some(res) = self.find_member_info_uncached(&Type::resolved(payload_ty), key, bind) {
            return Some(res);
        }
        let enum_name_str = self.resolve_bind_atom(bind, enum_name).to_string();
        let named = Type::named(
            enum_name_str,
            &mut *std::sync::Arc::make_mut(&mut self.ty_table),
        );
        self.find_member_info_uncached(&named, key, bind)
    }

    pub(super) fn member_named(
        &mut self,
        ty: &Type,
        name_atom: varn_core::Atom,
        origin_atom: Option<varn_core::Atom>,
        key: &str,
        bind: &BindResult,
    ) -> Option<(Type, Option<usize>)> {
        let name: Arc<str> = self.resolve_bind_atom(bind, name_atom);
        let origin: Option<Arc<str>> = origin_atom.map(|o| self.resolve_bind_atom(bind, o));
        if name.as_ref() == "*" {
            if let Some(origin_path) = &origin {
                let exports = if varn_binder::paths::is_known_module(origin_path) {
                    Some(self.resolver.stdlib_exports(origin_path))
                } else {
                    let mut visiting = Vec::new();
                    Some(self.resolver.module_exports(origin_path, &mut visiting))
                };
                if let Some(exports) = exports {
                    if let Some(sym) = exports.get(key) {
                        let mut sym_ty = sym.ty.unwrap_or(Type::Dynamic);
                        if let Some(origin) = &sym.origin_module {
                            let origin_str = self.resolve_bind_atom(bind, *origin).to_string();
                            let table = std::sync::Arc::make_mut(&mut self.ty_table);
                            let origin_atom = table.intern_name(&origin_str);
                            sym_ty = sym_ty.with_origin(origin_atom, table);
                        }
                        return Some((sym_ty, None));
                    }
                }
            }
            return None;
        }

        if name.as_ref() == varn_core::LangPrimitive::Str.name()
            && key == varn_core::MemberKey::Length.as_str()
        {
            return Some((Type::Int, None));
        }

        if let Some(owner) = self.foreign_owner(bind, &name, origin.as_deref()) {
            return self
                .find_member_info_uncached(ty, key, &owner)
                .map(|(t, sym)| (self.reintern_foreign_ty(&owner, t), sym));
        }

        let origin_modules: Vec<String> = origin.iter().map(|s| s.to_string()).collect();
        let is_enum = super::is_enum_type(self.resolver, bind, &name, &origin_modules);

        if is_enum {
            if key == varn_core::MemberKey::RawValue.as_str()
                || key == varn_core::MemberKey::Tag.as_str()
            {
                return Some((Type::Int, None));
            }
            if key == varn_core::MemberKey::Name.as_str()
                || key == varn_core::MemberKey::VariantName.as_str()
            {
                return Some((Type::Str, None));
            }

            let mut variants = Vec::new();
            if let Some(members) = bind.get_enum_members_local(name.as_ref()) {
                variants.extend(members.iter().map(|m| m.name.clone()));
            }
            if let Some(ext_bind) = self.resolver.find_bind_for_type(&name, &origin_modules) {
                if let Some(members) = ext_bind.get_enum_members_local(name.as_ref()) {
                    variants.extend(members.iter().map(|m| m.name.clone()));
                }
            }
            let mut found_tys = Vec::new();
            for v in &variants {
                if let Some(fields) = bind.sum_variant_fields.get(v) {
                    if let Some((_, ty)) = fields.iter().find(|(fname, _)| fname.as_ref() == key) {
                        found_tys.push(*ty);
                    }
                }
                if let Some(ext_bind) = self.resolver.find_bind_for_type(&name, &origin_modules) {
                    if let Some(fields) = ext_bind.sum_variant_fields.get(v) {
                        if let Some((_, ty)) =
                            fields.iter().find(|(fname, _)| fname.as_ref() == key)
                        {
                            let ty = *ty;
                            found_tys.push(self.reintern_foreign_ty(&ext_bind, ty));
                        }
                    }
                }
            }
            if !found_tys.is_empty() {
                return Some((
                    Type::union(
                        found_tys,
                        &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                    ),
                    None,
                ));
            }
        }
        if let Some(members) = bind.type_members.classes.get(&name) {
            if let Some(m) = members.members.iter().find(|m| m.name.as_ref() == key) {
                let ty = if m.is_optional {
                    Type::make_nullable(m.ty, &mut *std::sync::Arc::make_mut(&mut self.ty_table))
                } else {
                    m.ty
                };
                return Some((ty, m.symbol_id));
            }
        }
        if let Some(members) = bind.type_members.interfaces.get(&name) {
            if let Some(m) = members.iter().find(|m| m.name.as_ref() == key) {
                let ty = if m.is_optional {
                    Type::make_nullable(m.ty, &mut *std::sync::Arc::make_mut(&mut self.ty_table))
                } else {
                    m.ty
                };
                return Some((ty, m.symbol_id));
            }
        }
        if let Some(members) = bind.get_enum_members_local(name.as_ref()) {
            if let Some(m) = members.iter().find(|m| m.name.as_ref() == key) {
                return Some((m.ty, m.symbol_id));
            }
        }

        if let Some(ty) = bind
            .get_class_methods_for(name.as_ref())
            .and_then(|m| m.get(key))
        {
            return Some((*ty, None));
        }
        if let Some(b) = &bind.core {
            if let Some(members) = b.class_members.get(name.as_ref()) {
                if let Some(m) = members.members.iter().find(|m| m.name.as_ref() == key) {
                    return Some((m.ty, m.symbol_id));
                }
            }
            if let Some(members) = b.flattened_members.get(name.as_ref()) {
                if let Some(m) = members.iter().find(|m| m.name.as_ref() == key) {
                    return Some((m.ty, m.symbol_id));
                }
            }
            if let Some(methods) = b.class_methods.get(name.as_ref()) {
                if let Some(ty) = methods.get(key) {
                    return Some((*ty, None));
                }
            }
        }
        if let Some(parent) = bind.class_parents.get(&name) {
            let parent = parent.clone();
            let named = self.parent_type(&parent);
            return self.find_member_info_uncached(&named, key, bind);
        }

        let ext_bind_opt = self.resolver.find_bind_for_type(&name, &origin_modules);
        if let Some(ext_bind) = ext_bind_opt {
            if let Some(found) = self.search_named_in(&ext_bind, &name, key) {
                return Some(found);
            }
        } else if origin.is_none() {
            let resolver = self.resolver;
            let mut found = None;
            resolver.find_stdlib_bind(&mut |ext_bind| {
                found = self.search_named_in(ext_bind, &name, key);
                found.is_some()
            });
            if let Some(found) = found {
                return Some(found);
            }
        }
        None
    }

    fn search_named_in(
        &mut self,
        ext_bind: &BindResult,
        name: &Arc<str>,
        key: &str,
    ) -> Option<(Type, Option<usize>)> {
        if let Some(members) = ext_bind.type_members.classes.get(name) {
            if let Some(m) = members.members.iter().find(|m| m.name.as_ref() == key) {
                let ty = m.ty;
                return Some((self.reintern_foreign_ty(ext_bind, ty), m.symbol_id));
            }
        }
        if let Some(entry) = ext_bind.get_class_entry(name) {
            if let Some(m) = entry.members.iter().find(|m| m.name.as_ref() == key) {
                let ty = m.ty;
                return Some((self.reintern_foreign_ty(ext_bind, ty), m.symbol_id));
            }
        }
        if let Some(members) = ext_bind.type_members.interfaces.get(name) {
            if let Some(m) = members.iter().find(|m| m.name.as_ref() == key) {
                let ty = m.ty;
                return Some((self.reintern_foreign_ty(ext_bind, ty), m.symbol_id));
            }
        }
        if let Some(members) = ext_bind.get_enum_members_local(name.as_ref()) {
            if let Some(m) = members.iter().find(|m| m.name.as_ref() == key) {
                let ty = m.ty;
                return Some((self.reintern_foreign_ty(ext_bind, ty), m.symbol_id));
            }
        }
        if let Some(ty) = ext_bind
            .get_class_methods_for(name.as_ref())
            .and_then(|m| m.get(key))
        {
            let ty = *ty;
            return Some((self.reintern_foreign_ty(ext_bind, ty), None));
        }
        None
    }
}
