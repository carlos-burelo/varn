use varn_core::{Atom, TypeKind};

use varn_sem::scope::ScopeId;
use varn_sem::symbol::SymbolId;
use varn_sem::types::Type;

use super::Binder;

pub(crate) struct ArrayCandidate {
    sym_id: SymbolId,
    name: Atom,
    owner_scope: ScopeId,
    elem_ty: Option<Type>,
    conflict: bool,
    escaped: bool,
}

impl<'r> Binder<'r> {
    pub(crate) fn register_array_candidate(&mut self, sym_id: SymbolId, name: Atom) {
        self.array_watch.push(ArrayCandidate {
            sym_id,
            name,
            owner_scope: self.current,
            elem_ty: None,
            conflict: false,
            escaped: false,
        });
    }

    pub(crate) fn array_candidate_active(&self, name: Atom) -> bool {
        !self.array_watch.is_empty() && self.find_candidate(name).is_some()
    }

    fn find_candidate(&self, name: Atom) -> Option<&ArrayCandidate> {
        self.array_watch.iter().rev().find(|c| c.name == name)
    }

    fn find_candidate_mut(&mut self, name: Atom) -> Option<&mut ArrayCandidate> {
        self.array_watch.iter_mut().rev().find(|c| c.name == name)
    }

    pub(crate) fn record_array_write(&mut self, name: Atom, value_ty: &Type) {
        if self.array_watch.is_empty() {
            return;
        }
        let normalized = match self.ty_table.get(value_ty.0) {
            TypeKind::Primitive(varn_core::LangPrimitive::Int) => Some(Type::Int),
            TypeKind::Primitive(varn_core::LangPrimitive::Float) => Some(Type::Float),
            TypeKind::Primitive(_)
            | TypeKind::Builtin(_)
            | TypeKind::Literal(_)
            | TypeKind::This
            | TypeKind::Array(_)
            | TypeKind::Union(_)
            | TypeKind::Intersection(_)
            | TypeKind::Tuple(_)
            | TypeKind::Named(..)
            | TypeKind::Generic(..)
            | TypeKind::TemplateLiteral(_)
            | TypeKind::Fn(_)
            | TypeKind::Object(_)
            | TypeKind::Typeof(_)
            | TypeKind::KeyOf(_)
            | TypeKind::IndexedAccess { .. }
            | TypeKind::Mapped { .. }
            | TypeKind::Conditional { .. }
            | TypeKind::Infer(_)
            | TypeKind::EnumVariant { .. }
            | TypeKind::TypePredicate { .. } => None,
        };
        if let Some(c) = self.find_candidate_mut(name) {
            if c.escaped || c.conflict {
                return;
            }
            match normalized {
                None => c.conflict = true,
                Some(t) => match &c.elem_ty {
                    None => c.elem_ty = Some(t),
                    Some(existing) if *existing == t => {}
                    Some(_) => c.conflict = true,
                },
            }
        }
    }

    pub(crate) fn escape_array_candidate(&mut self, name: Atom) {
        if self.array_watch.is_empty() {
            return;
        }
        if let Some(c) = self.find_candidate_mut(name) {
            c.escaped = true;
        }
    }

    pub(crate) fn escape_all_open_array_candidates(&mut self) {
        for c in self.array_watch.iter_mut() {
            c.escaped = true;
        }
    }

    pub(crate) fn finalize_array_watch(&mut self, scope: ScopeId) {
        if self.array_watch.is_empty() {
            return;
        }
        let mut i = 0;
        while i < self.array_watch.len() {
            if self.array_watch[i].owner_scope == scope {
                let c = self.array_watch.remove(i);
                if !c.escaped && !c.conflict {
                    if let Some(elem) = c.elem_ty {
                        let offset = self.arena.get(c.sym_id).offset;
                        let array_ty =
                            Type::array(elem, &mut *std::sync::Arc::make_mut(&mut self.ty_table));
                        self.evolved_array_types.insert(offset, array_ty);
                    }
                }
            } else {
                i += 1;
            }
        }
    }
}
