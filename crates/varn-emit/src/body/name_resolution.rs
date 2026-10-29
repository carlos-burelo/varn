use super::context::FnEmitter;
use std::sync::Arc;
use varn_tir::{BackendTy, DynReason, LocalId, Resolution};

impl<'a> FnEmitter<'a> {
    pub(super) fn scopes_outward(&self) -> Vec<&str> {
        let Some(path) = self.namespace.as_deref() else {
            return Vec::new();
        };
        let mut out = vec![path];
        let mut rest = path;
        while let Some((outer, _)) = rest.rsplit_once("::") {
            out.push(outer);
            rest = outer;
        }
        out
    }

    pub(super) fn scoped_global(&self, name: &str) -> Option<u32> {
        self.scopes_outward().into_iter().find_map(|scope| {
            let key = crate::namespaces::scoped_name(Some(scope), name);
            if self.not_yet_bound.contains(&key) {
                return None;
            }
            self.m.globals.get(&key).copied()
        })
    }

    pub(super) fn known_fn(&self, name: varn_core::Atom) -> Option<(u32, u32)> {
        let bare = self.m.interner.resolve(name);
        let scoped = self.scopes_outward().into_iter().find_map(|scope| {
            let key = crate::namespaces::scoped_name(Some(scope), bare);
            self.m.scoped_fns.get(&key).copied()
        });
        scoped.or_else(|| self.m.fns.get(&name).copied())
    }

    pub(super) fn by_name(&self, name: Arc<str>, receiver: BackendTy) -> Resolution {
        Resolution::ByName {
            name,
            why: self.by_name_reason(receiver),
        }
    }

    pub(super) fn by_name_reason(&self, receiver: BackendTy) -> DynReason {
        match receiver.non_nullable(self.tt) {
            BackendTy::Dynamic(reason) => reason,
            BackendTy::Literal(_)
            | BackendTy::Int
            | BackendTy::Float
            | BackendTy::Bool
            | BackendTy::Char
            | BackendTy::Str
            | BackendTy::Bytes
            | BackendTy::Decimal
            | BackendTy::BigInt
            | BackendTy::Array(_)
            | BackendTy::Map(..)
            | BackendTy::Dict(_)
            | BackendTy::Set(_)
            | BackendTy::Tuple(_)
            | BackendTy::Enum(_)
            | BackendTy::Fn(_)
            | BackendTy::Nullable(_)
            | BackendTy::Void
            | BackendTy::Never => DynReason::CoreMember,
            BackendTy::Class(_) => DynReason::Unrecorded,
        }
    }

    pub(super) fn resolve_name(&mut self, name: &str) -> Resolution {
        for scope in self.scopes.iter().rev() {
            if let Some(id) = scope.get(name) {
                return Resolution::Local(*id);
            }
        }
        if let Some(i) = self.params.iter().position(|p| p.as_ref() == name) {
            return Resolution::Param(i as u32);
        }
        if self.outer_names.contains(name) {
            let idx = match self.captures.iter().position(|c| c.as_ref() == name) {
                Some(i) => i,
                None => {
                    self.captures.push(Arc::from(name));
                    self.captures.len() - 1
                }
            };
            return Resolution::Upvalue(idx as u32);
        }
        if let Some(slot) = self.scoped_global(name) {
            return Resolution::GlobalSlot(slot);
        }
        if let Some(&slot) = self.m.globals.get(name) {
            return Resolution::GlobalSlot(slot);
        }
        if let Some(idx) = varn_core::abi::native_global_index(name) {
            return Resolution::NativeGlobal(idx);
        }
        Resolution::ByName {
            name: Arc::from(name),
            why: DynReason::Declared,
        }
    }

    pub(super) fn declared_ty(&mut self, offset: u32) -> Option<BackendTy> {
        let resolved = self.m.declared_types.get(&offset)?;
        Some(crate::ty::lower_type(
            resolved,
            self.m.checker_table,
            self.m.interner,
            self.tt,
            self.m.names,
        ))
    }

    pub(super) fn bind_local(&mut self, name: Arc<str>, ty: BackendTy) -> LocalId {
        let id = LocalId(self.locals.len() as u32);
        self.locals.push(ty);
        self.scopes.last_mut().unwrap().insert(name, id);
        id
    }
}
