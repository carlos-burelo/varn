use crate::symbol::SymbolId;
use rustc_hash::FxHashMap;
use varn_core::Atom;
pub type ScopeId = usize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ScopeKind {
    Global,
    Module,
    Function,
    Block,
    Class,
    Interface,
    Namespace,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CheckerScope {
    pub kind: ScopeKind,
    pub parent: Option<ScopeId>,
    // `Atom` is a per-parse interned index and does not (and should not)
    // derive `serde::{Serialize, Deserialize}` — see
    // `binder::types::TypeMembers::objects` for the established rationale.
    #[serde(skip)]
    pub bindings: FxHashMap<Atom, SymbolId>,
    pub children: Vec<ScopeId>,
    pub ordered: Vec<SymbolId>,
}

impl CheckerScope {
    pub fn new(kind: ScopeKind, parent: Option<ScopeId>) -> Self {
        Self {
            kind,
            parent,
            bindings: FxHashMap::default(),
            children: Vec::new(),
            ordered: Vec::new(),
        }
    }

    pub fn define(&mut self, name: Atom, id: SymbolId) {
        self.bindings.insert(name, id);
        self.ordered.push(id);
    }

    pub fn lookup(&self, name: Atom) -> Option<SymbolId> {
        self.bindings.get(&name).copied()
    }

    pub fn resolve<'s>(&'s self, name: Atom, arena: &'s ScopeArena) -> Option<SymbolId> {
        let mut current = self;
        loop {
            if let Some(&id) = current.bindings.get(&name) {
                return Some(id);
            }
            {
                let parent_id = current.parent?;
                current = arena.get(parent_id)
            }
        }
    }
}

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ScopeArena {
    scopes: Vec<CheckerScope>,
}

impl ScopeArena {
    pub fn push(&mut self, scope: CheckerScope) -> ScopeId {
        let id = self.scopes.len();
        self.scopes.push(scope);
        id
    }

    pub fn get(&self, id: ScopeId) -> &CheckerScope {
        &self.scopes[id]
    }

    pub fn get_mut(&mut self, id: ScopeId) -> &mut CheckerScope {
        &mut self.scopes[id]
    }

    pub fn child(&mut self, kind: ScopeKind, parent: ScopeId) -> ScopeId {
        let child_id = self.push(CheckerScope::new(kind, Some(parent)));
        self.scopes[parent].children.push(child_id);
        child_id
    }

    pub fn global(&self) -> ScopeId {
        0
    }
}
