use super::Checker;
use crate::binder::{BindResult, BindView};
use crate::types::Type;
use std::sync::Arc;
use varn_core::ast::ExprId;

impl<'r> Checker<'r> {
    pub(crate) fn infer_type(&mut self, expr: ExprId, bind: &BindResult) -> Type {
        let key = (expr, self.current_scope, self.infer_env_rev);
        if let Some(ty) = self.infer_cache.get(&key) {
            return *ty;
        }

        let ty = self.infer_type_internal(expr, bind);

        self.infer_cache.insert(key, ty);
        ty
    }

    pub(crate) fn types_compatible_cached(
        &mut self,
        declared: &Type,
        inferred: &Type,
        bind: Option<&BindResult>,
    ) -> bool {
        let declared = &bind
            .and_then(|b| self.instantiate_interface(declared, inferred, b))
            .unwrap_or(*declared);
        let resolver = self.resolver;
        let view = bind.map(|b| BindView::new(b, resolver));
        super::compat::types_compatible_with_cache(
            declared,
            inferred,
            view.as_ref(),
            &mut self.compat_cache,
            &self.ty_table,
        )
    }

    fn instantiate_interface(
        &mut self,
        ty: &Type,
        inferred: &Type,
        bind: &BindResult,
    ) -> Option<Type> {
        use crate::types::TypeContext;
        let varn_core::TypeKind::Generic(name_atom, args_list, origin_atom) =
            self.ty_table.get(ty.0)
        else {
            return None;
        };
        if !matches!(
            self.ty_table.get(inferred.0),
            varn_core::TypeKind::Named(..) | varn_core::TypeKind::Object(_)
        ) {
            return None;
        }
        if let varn_core::TypeKind::Named(inferred_name, _) = self.ty_table.get(inferred.0) {
            if inferred_name == name_atom {
                return None;
            }
        }
        let name = self.resolve_bind_atom(bind, name_atom);
        let origin = origin_atom.map(|o| self.resolve_bind_atom(bind, o));
        let view = BindView::new(bind, self.resolver);
        if view.get_class_members(&name, origin.as_deref()).is_some() {
            return None;
        }
        let members = view.get_interface_members(&name, origin.as_deref())?;
        let args: Vec<Type> = self
            .ty_table
            .get_list(args_list)
            .iter()
            .map(|id| Type(*id, false))
            .collect();
        let mapping = crate::checker_expressions::members::member_util::generic_mapping(
            self.resolver,
            &name,
            &args,
            origin.as_ref(),
            bind,
        );
        if mapping.is_empty() {
            return None;
        }
        let table = Arc::make_mut(&mut self.ty_table);
        let object_members: Vec<_> = members
            .iter()
            .map(|cm| cm.as_object_member(table).map_generics(&mapping, table))
            .collect();
        Some(Type::object(object_members, table))
    }

    pub(crate) fn value_assignable_to(
        &mut self,
        target_ty: &Type,
        init_ty: &Type,
        init_expr: Option<varn_core::ast::ExprId>,
        bind: Option<&BindResult>,
    ) -> bool {
        if self.types_compatible_cached(target_ty, init_ty, bind) {
            return true;
        }
        super::compat::expr_satisfies_target_type(
            target_ty,
            init_ty,
            self.ast_arena,
            init_expr,
            &self.ty_table,
            bind.map(|b| &b.interner),
        )
    }

    pub(crate) fn resolve_bind_atom(&self, bind: &BindResult, atom: varn_core::Atom) -> Arc<str> {
        if let Some(s) = bind.interner.try_resolve(atom) {
            return Arc::from(s);
        }
        if let Some(s) = self.resolver.interner_snapshot().try_resolve(atom) {
            return Arc::from(s);
        }
        Arc::from(format!("<stale:{atom:?}>"))
    }

    pub(crate) fn reintern_foreign_ty(&mut self, bind: &BindResult, ty: Type) -> Type {
        if !self.ty_table.contains(ty.0) {
            std::sync::Arc::make_mut(&mut self.ty_table).absorb(&bind.ty_table);
        }
        if matches!(
            self.ty_table.get(ty.0),
            varn_core::TypeKind::Named(_, None) | varn_core::TypeKind::Generic(_, _, None)
        ) {
            let origin = self.resolver.intern(bind.source_file.as_ref());
            return ty.with_origin(origin, &mut *std::sync::Arc::make_mut(&mut self.ty_table));
        }
        ty
    }

    pub(crate) fn mark_infer_env_dirty(&mut self) {
        self.infer_env_rev = self.infer_env_rev.wrapping_add(1);
        if self.infer_cache.len() > 16_384 {
            self.infer_cache.clear();
        }
    }

    pub(crate) fn resolve_type_node_cached(
        &mut self,
        node: &varn_core::ast::TypeNode,
        bind: &BindResult,
    ) -> Type {
        let key = (node.range.start.offset, bind as *const BindResult as usize);
        if let Some(cached) = self.type_node_cache.get(&key) {
            return *cached;
        }
        let view = crate::binder::BindView::new(bind, self.resolver);
        let resolved = crate::binder::resolve_type_node(
            node,
            Some(&view),
            &mut *std::sync::Arc::make_mut(&mut self.ty_table),
        );
        self.type_node_cache.insert(key, resolved);
        resolved
    }

    pub(crate) fn symbol_type_params(
        &mut self,
        name: &str,
        kind: crate::symbol::SymbolKind,
        bind: &BindResult,
    ) -> Vec<Arc<str>> {
        let key = (Arc::from(name), symbol_kind_cache_key(kind));
        if let Some(cached) = self.symbol_type_params_cache.get(&key) {
            return cached.clone();
        }

        let resolved = if let Some(sid) = bind.interner.get(name).and_then(|atom| {
            bind.scopes
                .get(bind.global_scope)
                .resolve(atom, &bind.scopes)
        }) {
            let sym = bind.arena.get(sid);
            if sym.kind == kind {
                sym.type_params
                    .iter()
                    .map(|a| Arc::from(bind.interner.resolve(*a)))
                    .collect()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        self.symbol_type_params_cache.insert(key, resolved.clone());
        resolved
    }

    pub(crate) fn symbol_type_params_any(
        &mut self,
        name: &str,
        bind: &BindResult,
    ) -> Vec<Arc<str>> {
        let key = (Arc::from(name), 255);
        if let Some(cached) = self.symbol_type_params_cache.get(&key) {
            return cached.clone();
        }

        let resolved = if let Some(sid) = bind.interner.get(name).and_then(|atom| {
            bind.scopes
                .get(bind.global_scope)
                .resolve(atom, &bind.scopes)
        }) {
            bind.arena
                .get(sid)
                .type_params
                .iter()
                .map(|a| Arc::from(bind.interner.resolve(*a)))
                .collect()
        } else {
            bind.core
                .as_ref()
                .and_then(|b| b.class_type_params.get(name))
                .cloned()
                .unwrap_or_default()
        };

        self.symbol_type_params_cache.insert(key, resolved.clone());
        resolved
    }
}

fn symbol_kind_cache_key(kind: crate::symbol::SymbolKind) -> u8 {
    match kind {
        crate::symbol::SymbolKind::Var => 0,
        crate::symbol::SymbolKind::Let => 1,
        crate::symbol::SymbolKind::Const => 2,
        crate::symbol::SymbolKind::Function => 3,
        crate::symbol::SymbolKind::Class => 4,
        crate::symbol::SymbolKind::Interface => 5,
        crate::symbol::SymbolKind::TypeAlias => 6,
        crate::symbol::SymbolKind::Enum => 7,
        crate::symbol::SymbolKind::Parameter => 8,
        crate::symbol::SymbolKind::Property => 9,
        crate::symbol::SymbolKind::Method => 10,
        crate::symbol::SymbolKind::TypeParameter => 11,
        crate::symbol::SymbolKind::Namespace => 12,
        crate::symbol::SymbolKind::Struct => 13,
        crate::symbol::SymbolKind::Extension => 14,
        crate::symbol::SymbolKind::EnumMember => 15,
    }
}
