use super::model::PortableModule;
use crate::bind::{BindResult, TypeMembers};
use crate::scope::{CheckerScope, ScopeArena};
use crate::symbol::{Symbol, SymbolArena};
use crate::types::{CheckerTyId, CheckerTyTable, ClassMemberInfo};
use std::collections::BTreeSet;
use varn_core::{Atom, AtomInterner};

#[derive(Default)]
struct Roots {
    types: Vec<CheckerTyId>,
    atoms: BTreeSet<Atom>,
}

impl Roots {
    fn symbol(&mut self, s: &Symbol) {
        self.types.extend(s.ty.map(|t| t.0));
        self.types.extend(
            s.type_param_constraints
                .iter()
                .chain(&s.type_param_defaults)
                .flatten()
                .map(|t| t.0),
        );
        self.atoms.insert(s.name);
        self.atoms.extend(s.doc);
        self.atoms.extend(s.type_params.iter().copied());
        self.atoms.extend(s.origin_module);
        self.atoms.extend(s.re_export_path.iter().copied());
        self.atoms.extend(s.original_name);
    }

    fn member(&mut self, m: &ClassMemberInfo) {
        self.types.push(m.ty.0);
        for child in &m.members {
            self.member(child);
        }
    }

    fn type_members(&mut self, tm: &TypeMembers) {
        for m in tm.classes.values() {
            self.member(m);
        }
        self.atoms.extend(tm.objects.keys().copied());
        for list in tm
            .interfaces
            .values()
            .chain(tm.objects.values())
            .chain(tm.enums.values())
            .chain(tm.namespaces.values())
            .chain(tm.flattened.values())
        {
            for m in list {
                self.member(m);
            }
        }
        for accessors in tm.getters.values().chain(tm.setters.values()) {
            self.types.extend(accessors.values().map(|t| t.0));
        }
    }

    fn bind(&mut self, bind: &BindResult) {
        for s in bind.arena.all() {
            self.symbol(s);
        }
        for id in 0..bind.scopes.len() {
            self.atoms
                .extend(bind.scopes.get(id).bindings.keys().copied());
        }
        self.type_members(&bind.type_members);
        for parent in bind
            .class_parents
            .values()
            .chain(bind.class_interfaces.values().flatten())
        {
            self.types.extend(parent.args.iter().map(|t| t.0));
        }
        for methods in bind.class_methods.values() {
            self.types.extend(methods.values().map(|t| t.0));
        }
        for fields in bind.sum_variant_fields.values() {
            self.types.extend(fields.iter().map(|(_, t)| t.0));
        }
    }
}

impl PortableModule {
    pub(crate) fn from_live(
        exports: &crate::exports::ExportMap,
        bind: &BindResult,
        names: &AtomInterner,
        table: &CheckerTyTable,
    ) -> Self {
        let bind = project_for_importers(bind);
        let mut roots = Roots::default();
        for s in exports.values() {
            roots.symbol(s);
        }
        roots.bind(&bind);
        let types = table.slice(roots.types);
        roots.atoms.extend(types.atoms());
        let names = roots
            .atoms
            .iter()
            .filter_map(|a| names.try_resolve(*a))
            .map(Box::from)
            .collect();
        Self {
            exports: exports.clone(),
            bind,
            names,
            types,
        }
    }
}

fn injected_global(bind: &BindResult, symbol: &Symbol) -> bool {
    symbol
        .origin_module
        .and_then(|origin| bind.interner.try_resolve(origin))
        .is_some_and(|origin| {
            matches!(
                varn_core::ModuleId::from_canonical_str(origin),
                varn_core::ModuleId::Core(_)
            ) && origin != bind.source_file.as_ref()
        })
}

fn project_for_importers(bind: &BindResult) -> BindResult {
    let global = bind.scopes.get(bind.global_scope);
    let own = |id: &usize| !injected_global(bind, bind.arena.get(*id));
    let mut arena = SymbolArena::default();
    let mut remap = rustc_hash::FxHashMap::default();
    let mut keep = |id: usize, arena: &mut SymbolArena| {
        *remap
            .entry(id)
            .or_insert_with(|| arena.push(bind.arena.get(id).clone()))
    };
    let mut scope = CheckerScope::new(global.kind, None);
    scope.ordered = global
        .ordered
        .iter()
        .filter(|id| own(id))
        .map(|&id| keep(id, &mut arena))
        .collect();
    let mut bindings: Vec<_> = global.bindings.iter().filter(|(_, id)| own(id)).collect();
    bindings.sort_by_key(|(atom, _)| **atom);
    for (atom, &id) in bindings {
        scope.bindings.insert(*atom, keep(id, &mut arena));
    }
    let mut scopes = ScopeArena::default();
    let global_scope = scopes.push(scope);

    let mut projected = bind.clone();
    projected.arena = arena;
    projected.scopes = scopes;
    projected.global_scope = global_scope;
    strip_symbol_ids(&mut projected.type_members);
    projected
}

fn strip_symbol_ids(tm: &mut TypeMembers) {
    fn strip(m: &mut ClassMemberInfo) {
        m.symbol_id = None;
        m.members.iter_mut().for_each(strip);
    }
    tm.classes.values_mut().for_each(strip);
    for list in tm
        .interfaces
        .values_mut()
        .chain(tm.objects.values_mut())
        .chain(tm.enums.values_mut())
        .chain(tm.namespaces.values_mut())
        .chain(tm.flattened.values_mut())
    {
        list.iter_mut().for_each(strip);
    }
}
