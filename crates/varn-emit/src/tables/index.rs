use super::classes::build_classes;
use super::enums::build_enums;
use super::signatures::seed_signatures;
use crate::ty::{lower_type, NameResolver, ScopedTypeParams};
use rustc_hash::FxHashMap;
use std::sync::Arc;
use varn_sem::bind::BindResult;
use varn_tir::{BackendTy, ClassId, EnumId};
use varn_tir::{ClassInfo, EnumInfo, Signature, TyTable, VariantInfo};

#[derive(Default)]
pub struct NameIndex {
    module: Arc<str>,
    classes: FxHashMap<Arc<str>, ClassId>,
    enums: FxHashMap<Arc<str>, EnumId>,
    foreign_enums: FxHashMap<(Arc<str>, Arc<str>), EnumId>,
    foreign_classes: FxHashMap<(Arc<str>, Arc<str>), ClassId>,
    aliases: FxHashMap<Arc<str>, varn_sem::types::Type>,
    type_params: rustc_hash::FxHashSet<Arc<str>>,
}

impl NameResolver for NameIndex {
    fn class_id(&self, name: &str) -> Option<ClassId> {
        self.classes.get(name).copied()
    }
    fn enum_id(&self, name: &str) -> Option<EnumId> {
        self.enums.get(name).copied()
    }
    fn foreign_enum_id(&self, name: &str, origin: &str) -> Option<EnumId> {
        self.foreign_enums
            .get(&(Arc::from(name), Arc::from(origin)))
            .copied()
    }
    fn foreign_class_id(&self, name: &str, origin: &str) -> Option<ClassId> {
        self.foreign_classes
            .get(&(Arc::from(name), Arc::from(origin)))
            .copied()
    }
    fn is_local_origin(&self, origin: &str) -> bool {
        origin == self.module.as_ref()
    }
    fn alias(&self, name: &str) -> Option<varn_sem::types::Type> {
        self.aliases.get(name).copied()
    }
    fn is_type_param(&self, name: &str) -> bool {
        self.type_params.contains(name)
    }
}

impl NameIndex {
    pub fn parent_class_id(&self, parent: &varn_sem::bind::ClassParent) -> Option<ClassId> {
        if let Some(id) = self.classes.get(parent.name.as_ref()).copied() {
            return Some(id);
        }
        if let Some(origin) = parent.origin.as_deref()
            && let Some(id) = self
                .foreign_classes
                .get(&(parent.name.clone(), Arc::from(origin)))
                .copied()
        {
            return Some(id);
        }
        self.foreign_classes
            .iter()
            .find(|((name, _), _)| name.as_ref() == parent.name.as_ref())
            .map(|(_, &id)| id)
    }

    pub fn foreign_parent_info<'a>(
        &'a self,
        id: ClassId,
        local_count: usize,
        foreign_infos: &'a [ClassInfo],
    ) -> Option<&'a ClassInfo> {
        let idx = id.0 as usize;
        if idx >= local_count {
            foreign_infos.get(idx - local_count)
        } else {
            None
        }
    }
}

pub struct Tables {
    pub classes: Vec<ClassInfo>,
    pub class_origins: Vec<Option<Arc<str>>>,
    pub enums: Vec<EnumInfo>,
    pub signatures: Vec<Signature>,
    pub names: NameIndex,
}

pub fn build(
    bind: &BindResult,
    desugar: &varn_sem::output::Desugarings,
    tt: &mut TyTable,
) -> Tables {
    let foreign = &desugar.foreign_enums;
    let mut foreign_classes_sorted: Vec<&varn_sem::output::ForeignClass> =
        desugar.foreign_classes.iter().collect();
    foreign_classes_sorted.sort_by(|a, b| (&a.origin, &a.name).cmp(&(&b.origin, &b.name)));
    let mut class_names: Vec<Arc<str>> = bind.type_members.classes.keys().cloned().collect();
    class_names.sort();
    class_names.retain(|n| !bind.type_members.enums.contains_key(n));

    let mut enum_names: Vec<Arc<str>> = bind
        .type_members
        .enums
        .keys()
        .chain(bind.sum_type_variants.keys())
        .cloned()
        .collect();
    enum_names.sort();
    enum_names.dedup();
    class_names.retain(|n| !bind.sum_type_variants.contains_key(n));

    let mut names = NameIndex {
        module: Arc::from(bind.source_file.as_ref()),
        ..NameIndex::default()
    };
    let global = bind.scopes.get(bind.global_scope);
    for symbol in bind.arena.all() {
        if symbol.kind == varn_sem::symbol::SymbolKind::TypeParameter {
            names
                .type_params
                .insert(Arc::from(bind.interner.resolve(symbol.name)));
        }
        if symbol.kind != varn_sem::symbol::SymbolKind::TypeAlias || !symbol.type_params.is_empty()
        {
            continue;
        }
        let declared = global
            .resolve(symbol.name, &bind.scopes)
            .map(|id| bind.arena.get(id))
            .filter(|found| found.kind == varn_sem::symbol::SymbolKind::TypeAlias);
        if let Some(target) = declared.and_then(|found| found.ty) {
            names
                .aliases
                .insert(Arc::from(bind.interner.resolve(symbol.name)), target);
        }
    }
    for (i, f) in foreign.iter().enumerate() {
        names.foreign_enums.insert(
            (f.name.clone(), f.origin.clone()),
            EnumId((enum_names.len() + i) as u32),
        );
    }
    for (i, n) in class_names.iter().enumerate() {
        names.classes.insert(n.clone(), ClassId(i as u32));
    }
    for (i, f) in foreign_classes_sorted.iter().enumerate() {
        names.foreign_classes.insert(
            (f.name.clone(), f.origin.clone()),
            ClassId((class_names.len() + i) as u32),
        );
    }
    for (i, n) in enum_names.iter().enumerate() {
        names.enums.insert(n.clone(), EnumId(i as u32));
    }

    let mut signatures = seed_signatures();
    let mut foreign_parent_infos: Vec<ClassInfo> = Vec::with_capacity(foreign_classes_sorted.len());
    for f in &foreign_classes_sorted {
        let mut fields: Vec<(Arc<str>, BackendTy)> = Vec::with_capacity(f.fields.len());
        for field in &f.fields {
            let inner =
                crate::ty::lower_type(&field.ty, &bind.ty_table, &bind.interner, tt, &names);
            let bt = if field.optional && !matches!(inner, BackendTy::Nullable(_)) {
                BackendTy::Nullable(tt.intern(inner))
            } else {
                inner
            };
            fields.push((field.name.clone(), bt));
        }
        let accessors: Vec<(Arc<str>, varn_tir::SigId)> = f
            .accessors
            .iter()
            .map(|accessor| {
                let sig = super::signatures::accessor_signature(
                    accessor.kind,
                    &accessor.ty,
                    &bind.ty_table,
                    &bind.interner,
                    tt,
                    &names,
                    &mut signatures,
                );
                (Arc::from(accessor.kind.key(&accessor.name)), sig)
            })
            .collect();
        let methods: Vec<(Arc<str>, varn_tir::SigId)> = f
            .methods
            .iter()
            .map(|method| {
                let sig = super::intern_signature(
                    &method.ty,
                    &bind.ty_table,
                    &bind.interner,
                    tt,
                    &names,
                    &mut signatures,
                );
                (method.name.clone(), sig)
            })
            .collect();
        let mut info = ClassInfo::new_with_methods(
            f.name.clone(),
            varn_tir::Ancestry::Root,
            fields,
            accessors.into_iter().chain(methods).collect(),
            Vec::new(),
            tt,
        );
        info.constructor = f.constructor.map(|ctor| {
            super::intern_signature(
                &ctor,
                &bind.ty_table,
                &bind.interner,
                tt,
                &names,
                &mut signatures,
            )
        });
        info.import_binding = f.import_binding.clone();
        foreign_parent_infos.push(info);
    }
    let mut classes = build_classes(
        bind,
        &bind.ty_table,
        &bind.interner,
        tt,
        &names,
        &class_names,
        &desugar.foreign_inherited_fields,
        &foreign_parent_infos,
        &mut signatures,
    );
    classes.extend(foreign_parent_infos);
    for (i, f) in foreign_classes_sorted.iter().enumerate() {
        if let Some((name, origin)) = &f.parent {
            classes[class_names.len() + i].parent = names.foreign_class_id(name, origin);
        }
    }
    let mut class_origins: Vec<Option<Arc<str>>> = vec![None; class_names.len()];
    class_origins.extend(
        foreign_classes_sorted
            .iter()
            .map(|f| Some(f.origin.clone())),
    );
    let mut enums = build_enums(
        bind,
        &bind.ty_table,
        &bind.interner,
        tt,
        &names,
        &enum_names,
        &mut signatures,
    );
    enums.extend(foreign.iter().map(|f| {
        let scoped = ScopedTypeParams {
            inner: &names,
            params: &f.type_params,
        };
        EnumInfo {
            name: f.name.clone(),
            binding: f.binding.clone(),
            members: super::enums::enum_members(
                f.members.iter().map(|m| (m.kind, &m.name, &m.ty)),
                &bind.ty_table,
                &bind.interner,
                tt,
                &scoped,
                &mut signatures,
            ),
            variants: f
                .variants
                .iter()
                .enumerate()
                .map(|(tag, (name, fields))| VariantInfo {
                    name: name.clone(),
                    tag: tag as u16,
                    payload: fields
                        .iter()
                        .map(|ty| lower_type(ty, &bind.ty_table, &bind.interner, tt, &scoped))
                        .collect(),
                })
                .collect(),
        }
    }));

    Tables {
        classes,
        class_origins,
        enums,
        signatures,
        names,
    }
}
