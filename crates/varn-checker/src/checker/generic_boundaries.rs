use super::Checker;
use std::collections::BTreeSet;
use varn_core::{Atom, TypeKind};
use varn_sem::bind::BindResult;
use varn_sem::scope::ScopeKind;
use varn_sem::symbol::SymbolKind;
use varn_sem::types::Type;

impl Checker<'_> {
    pub(crate) fn open_parameters(&self, ty: Type, bind: &BindResult) -> BTreeSet<Atom> {
        let mut pending = vec![ty];
        let mut seen = BTreeSet::new();
        let mut out = BTreeSet::new();
        while let Some(ty) = pending.pop() {
            if !seen.insert(ty.0) {
                continue;
            }
            if let TypeKind::Named(name, None) = self.ty_table.get(ty.0) {
                let scoped = bind
                    .scopes
                    .get(self.current_scope)
                    .resolve(name, &bind.scopes)
                    .is_some_and(|sid| bind.arena.get(sid).kind == SymbolKind::TypeParameter);
                if scoped
                    || self
                        .active_type_params
                        .contains(self.resolve_bind_atom(bind, name).as_ref())
                {
                    out.insert(name);
                }
            }
            pending.extend(ty.components(&self.ty_table));
        }
        out
    }

    pub(crate) fn unchecked_generic_conversion(
        &self,
        target: Type,
        given: Type,
        bind: &BindResult,
    ) -> bool {
        if target.0 == given.0 || self.open_parameters(target, bind).is_empty() {
            return false;
        }
        let mut pending = vec![given];
        let mut seen = BTreeSet::new();
        while let Some(ty) = pending.pop() {
            if !seen.insert(ty.0) {
                continue;
            }
            if ty.is_dynamic() {
                return true;
            }
            pending.extend(ty.components(&self.ty_table));
        }
        false
    }

    pub(crate) fn can_defer_decoder_return(&self, returned: Type, bind: &BindResult) -> bool {
        if self.in_async || matches!(self.ty_table.get(returned.0), TypeKind::Generic(..)) {
            return false;
        }
        let params = self.open_parameters(returned, bind);
        if params.is_empty() {
            return false;
        }
        let mut scope_id = self.current_scope;
        let scope = loop {
            let scope = bind.scopes.get(scope_id);
            if scope.kind == ScopeKind::Function {
                break scope;
            }
            let Some(parent) = scope.parent else {
                return false;
            };
            scope_id = parent;
        };
        if !params.iter().all(|name| {
            scope
                .lookup(*name)
                .is_some_and(|sid| bind.arena.get(sid).kind == SymbolKind::TypeParameter)
        }) {
            return false;
        }
        !scope.ordered.iter().any(|sid| {
            let symbol = bind.arena.get(*sid);
            symbol.kind == SymbolKind::Parameter
                && symbol
                    .ty
                    .is_some_and(|ty| !self.open_parameters(ty, bind).is_disjoint(&params))
        })
    }
}
