use super::classes::build_classes;
use super::enums::build_enums;
use super::signatures::seed_signatures;
use crate::binder::BindResult;
use crate::emit::ty::NameResolver;
use rustc_hash::FxHashMap;
use std::sync::Arc;
use varn_tir::{BackendTy, ClassId, EnumId};
use varn_tir::{ClassInfo, EnumInfo, Signature, TyTable, VariantInfo};

#[derive(Default)]
pub struct NameIndex {
    module: Arc<str>,
    classes: FxHashMap<Arc<str>, ClassId>,
    enums: FxHashMap<Arc<str>, EnumId>,
    foreign_enums: FxHashMap<(Arc<str>, Arc<str>), EnumId>,
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
    fn is_local_origin(&self, origin: &str) -> bool {
        origin == self.module.as_ref()
    }
}

pub struct Tables {
    pub classes: Vec<ClassInfo>,
    pub enums: Vec<EnumInfo>,
    pub signatures: Vec<Signature>,
    pub names: NameIndex,
}

pub fn build(
    bind: &BindResult,
    foreign: &[crate::checker::ForeignEnum],
    tt: &mut TyTable,
) -> Tables {
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
    for (i, f) in foreign.iter().enumerate() {
        names.foreign_enums.insert(
            (f.name.clone(), f.origin.clone()),
            EnumId((enum_names.len() + i) as u32),
        );
    }
    for (i, n) in class_names.iter().enumerate() {
        names.classes.insert(n.clone(), ClassId(i as u32));
    }
    for (i, n) in enum_names.iter().enumerate() {
        names.enums.insert(n.clone(), EnumId(i as u32));
    }

    let mut signatures = seed_signatures();
    let classes = build_classes(
        bind,
        &bind.ty_table,
        &bind.interner,
        tt,
        &names,
        &class_names,
        &mut signatures,
    );
    let mut enums = build_enums(
        bind,
        &bind.ty_table,
        &bind.interner,
        tt,
        &names,
        &enum_names,
    );
    enums.extend(foreign.iter().map(|f| {
        EnumInfo {
            name: f.name.clone(),
            variants: f
                .variants
                .iter()
                .enumerate()
                .map(|(tag, (name, fields))| VariantInfo {
                    name: name.clone(),
                    tag: tag as u16,
                    payload: vec![
                        BackendTy::Dynamic(varn_tir::DynReason::NotYetSupported);
                        *fields
                    ],
                })
                .collect(),
        }
    }));

    Tables {
        classes,
        enums,
        signatures,
        names,
    }
}
