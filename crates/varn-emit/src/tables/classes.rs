use super::signatures::{accessor_signature, intern_signature};
use super::NameIndex;
use crate::ty::{lower_type, NameResolver};
use rustc_hash::FxHashMap;
use std::sync::Arc;
use varn_core::AtomInterner;
use varn_sem::bind::BindResult;
use varn_sem::types::{CheckerTyTable, ClassMemberKind};
use varn_tir::{Ancestry, BackendTy, ClassInfo, DynReason, Signature, TyTable};

type ForeignFields = std::collections::BTreeMap<Arc<str>, Vec<varn_sem::output::InheritedField>>;
pub(super) fn build_classes(
    bind: &BindResult,
    table: &CheckerTyTable,
    interner: &AtomInterner,
    tt: &mut TyTable,
    names: &NameIndex,
    class_names: &[Arc<str>],
    foreign: &ForeignFields,
    foreign_parent_infos: &[ClassInfo],
    signatures: &mut Vec<Signature>,
) -> Vec<ClassInfo> {
    let mut built: Vec<Option<ClassInfo>> = vec![None; class_names.len()];
    for i in 0..class_names.len() {
        build_with_parents(
            bind,
            table,
            interner,
            tt,
            names,
            class_names,
            foreign,
            foreign_parent_infos,
            i,
            signatures,
            &mut built,
        );
    }
    built
        .into_iter()
        .map(|c| c.expect("every class is built"))
        .collect()
}
fn build_with_parents(
    bind: &BindResult,
    table: &CheckerTyTable,
    interner: &AtomInterner,
    tt: &mut TyTable,
    names: &NameIndex,
    class_names: &[Arc<str>],
    foreign: &ForeignFields,
    foreign_parent_infos: &[ClassInfo],
    i: usize,
    signatures: &mut Vec<Signature>,
    built: &mut [Option<ClassInfo>],
) {
    let mut chain = vec![i];
    let mut cur = i;
    while let Some(p) = bind
        .class_parents
        .get(&class_names[cur])
        .and_then(|p| names.parent_class_id(p))
        .map(|id| id.0 as usize)
    {
        if p >= class_names.len() || built[p].is_some() || chain.contains(&p) {
            break;
        }
        chain.push(p);
        cur = p;
    }
    for &c in chain.iter().rev() {
        if built[c].is_none() {
            let info = build_one_class(
                bind,
                table,
                interner,
                tt,
                names,
                class_names,
                &class_names[c],
                foreign.get(&class_names[c]).map(Vec::as_slice),
                foreign_parent_infos,
                signatures,
                built,
            );
            built[c] = Some(info);
        }
    }
}
fn build_one_class(
    bind: &BindResult,
    table: &CheckerTyTable,
    interner: &AtomInterner,
    tt: &mut TyTable,
    names: &NameIndex,
    class_names: &[Arc<str>],
    name: &Arc<str>,
    foreign: Option<&[varn_sem::output::InheritedField]>,
    foreign_parent_infos: &[ClassInfo],
    signatures: &mut Vec<Signature>,
    built: &[Option<ClassInfo>],
) -> ClassInfo {
    let parent_name = bind.class_parents.get(name);
    let parent_id = parent_name.and_then(|p| names.parent_class_id(p));
    let parent_info = parent_id.and_then(|id| {
        let idx = id.0 as usize;
        if idx < class_names.len() {
            built.get(idx)?.as_ref()
        } else {
            names.foreign_parent_info(id, class_names.len(), foreign_parent_infos)
        }
    });
    let mut inherited_fields: FxHashMap<Arc<str>, ()> = parent_info
        .map(|p| p.fields.iter().map(|f| (f.name.clone(), ())).collect())
        .unwrap_or_default();

    let foreign_prefix: Vec<(Arc<str>, BackendTy)> = foreign
        .filter(|_| parent_name.is_some() && parent_id.is_none())
        .unwrap_or_default()
        .iter()
        .map(|f| {
            inherited_fields.insert(f.name.clone(), ());
            let ty = field_backend_ty(f.ty, f.optional, table, interner, tt, names);
            (f.name.clone(), ty)
        })
        .collect();

    let mut fields: Vec<(Arc<str>, BackendTy)> = Vec::new();
    let members = bind
        .type_members
        .classes
        .get(name)
        .map(|e| e.members.as_slice())
        .unwrap_or(&[]);

    let statics = build_statics(
        members
            .iter()
            .filter(|m| m.is_static)
            .map(|m| (m.kind, &m.name, &m.ty, m.is_optional)),
        table,
        interner,
        tt,
        names,
        signatures,
    );

    let mut seen_field: FxHashMap<Arc<str>, ()> = FxHashMap::default();
    let mut method_names: Vec<Arc<str>> = Vec::new();
    let mut method_sig: FxHashMap<Arc<str>, varn_tir::SigId> = FxHashMap::default();

    let mut constructor = None;
    let mut push_method = |key: Arc<str>, sig: varn_tir::SigId, order: &mut Vec<Arc<str>>| {
        if method_sig.insert(key.clone(), sig).is_none() {
            order.push(key);
        }
    };

    for m in members {
        if m.is_static {
            continue;
        }
        match m.kind {
            ClassMemberKind::Property | ClassMemberKind::Variable => {
                if !inherited_fields.contains_key(&m.name)
                    && seen_field.insert(m.name.clone(), ()).is_none()
                {
                    let field_bt =
                        field_backend_ty(m.ty, m.is_optional, table, interner, tt, names);
                    fields.push((m.name.clone(), field_bt));
                }
            }
            ClassMemberKind::Method | ClassMemberKind::Function => {
                let sig = intern_signature(&m.ty, table, interner, tt, names, signatures);
                push_method(m.name.clone(), sig, &mut method_names);
            }
            ClassMemberKind::Constructor => {
                constructor = Some(intern_signature(
                    &m.ty, table, interner, tt, names, signatures,
                ));
            }
            ClassMemberKind::Getter => {
                let sig = accessor_signature(
                    varn_core::AccessorKind::Get,
                    &m.ty,
                    table,
                    interner,
                    tt,
                    names,
                    signatures,
                );
                push_method(
                    Arc::from(varn_core::AccessorKind::Get.key(&m.name)),
                    sig,
                    &mut method_names,
                );
            }
            ClassMemberKind::Setter => {
                let sig = accessor_signature(
                    varn_core::AccessorKind::Set,
                    &m.ty,
                    table,
                    interner,
                    tt,
                    names,
                    signatures,
                );
                push_method(
                    Arc::from(varn_core::AccessorKind::Set.key(&m.name)),
                    sig,
                    &mut method_names,
                );
            }
            ClassMemberKind::Class
            | ClassMemberKind::Interface
            | ClassMemberKind::Namespace
            | ClassMemberKind::Enum
            | ClassMemberKind::Struct => {}
        }
    }

    let methods: Vec<(Arc<str>, varn_tir::SigId)> = method_names
        .into_iter()
        .map(|n| (n.clone(), method_sig[&n]))
        .collect();

    let ancestry = match parent_id.zip(parent_info) {
        Some((id, info)) => Ancestry::Local(id, info),
        None if !foreign_prefix.is_empty() => Ancestry::Foreign(foreign_prefix),
        None => Ancestry::Root,
    };
    let mut info =
        ClassInfo::new_with_methods(name.clone(), ancestry, fields, methods, statics, tt);
    info.constructor = constructor;
    info
}

pub(super) type StaticMember<'a> = (
    ClassMemberKind,
    &'a Arc<str>,
    &'a varn_sem::types::Type,
    bool,
);

pub(super) fn build_statics<'a>(
    members: impl Iterator<Item = StaticMember<'a>> + Clone,
    table: &CheckerTyTable,
    interner: &AtomInterner,
    tt: &mut TyTable,
    names: &dyn NameResolver,
    signatures: &mut Vec<Signature>,
) -> Vec<(Arc<str>, BackendTy, Option<varn_tir::SigId>)> {
    let mut statics: Vec<(Arc<str>, BackendTy, Option<varn_tir::SigId>)> = Vec::new();
    for (kind, name, ty, optional) in members.clone() {
        match kind {
            ClassMemberKind::Property | ClassMemberKind::Variable => {
                if statics.iter().any(|(n, _, _)| n == name) {
                    continue;
                }
                let static_bt = field_backend_ty(*ty, optional, table, interner, tt, names);
                statics.push((name.clone(), static_bt, None));
            }
            ClassMemberKind::Method
            | ClassMemberKind::Function
            | ClassMemberKind::Constructor
            | ClassMemberKind::Getter
            | ClassMemberKind::Setter
            | ClassMemberKind::Class
            | ClassMemberKind::Interface
            | ClassMemberKind::Namespace
            | ClassMemberKind::Enum
            | ClassMemberKind::Struct => {}
        }
    }
    for (kind, name, ty, _) in members {
        match kind {
            ClassMemberKind::Method | ClassMemberKind::Function => {
                if statics.iter().any(|(n, _, _)| n == name) {
                    continue;
                }
                let sig = intern_signature(ty, table, interner, tt, names, signatures);
                statics.push((
                    name.clone(),
                    BackendTy::Dynamic(DynReason::Declared),
                    Some(sig),
                ));
            }
            ClassMemberKind::Property
            | ClassMemberKind::Variable
            | ClassMemberKind::Constructor
            | ClassMemberKind::Getter
            | ClassMemberKind::Setter
            | ClassMemberKind::Class
            | ClassMemberKind::Interface
            | ClassMemberKind::Namespace
            | ClassMemberKind::Enum
            | ClassMemberKind::Struct => {}
        }
    }
    statics
}

fn field_backend_ty(
    ty: varn_sem::types::Type,
    optional: bool,
    table: &CheckerTyTable,
    interner: &AtomInterner,
    tt: &mut TyTable,
    names: &dyn NameResolver,
) -> BackendTy {
    let inner = lower_type(&ty, table, interner, tt, names);
    if optional && !matches!(inner, BackendTy::Nullable(_)) {
        BackendTy::Nullable(tt.intern(inner))
    } else {
        inner
    }
}

pub(crate) fn inherited_constructor(
    classes: &[ClassInfo],
    class: varn_tir::ClassId,
) -> Option<varn_tir::SigId> {
    let mut current = Some(class);
    for _ in 0..=classes.len() {
        let info = classes.get(current?.0 as usize)?;
        if let Some(sig) = info.constructor {
            return Some(sig);
        }
        current = info.parent;
    }
    None
}
