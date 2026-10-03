use super::cache_types::PortableModule;
use crate::binder::{BindResult, TypeMembers};
use crate::symbol::Symbol;
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
        self.types
            .extend(s.type_param_constraints.iter().flatten().map(|t| t.0));
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
        for methods in bind.class_methods.values() {
            self.types.extend(methods.values().map(|t| t.0));
        }
        for fields in bind.sum_variant_fields.values() {
            self.types.extend(fields.iter().map(|(_, t)| t.0));
        }
    }
}

impl PortableModule {
    pub(super) fn from_live(
        exports: &super::super::ExportMap,
        bind: &BindResult,
        names: &AtomInterner,
        table: &CheckerTyTable,
    ) -> Self {
        let mut roots = Roots::default();
        for s in exports.values() {
            roots.symbol(s);
        }
        roots.bind(bind);
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
            bind: bind.clone(),
            names,
            types,
        }
    }
}
