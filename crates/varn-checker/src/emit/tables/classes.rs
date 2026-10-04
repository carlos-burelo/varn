use super::signatures::intern_signature;
use super::NameIndex;
use crate::binder::BindResult;
use crate::emit::ty::{lower_type, NameResolver};
use crate::types::{CheckerTyTable, ClassMemberKind};
use rustc_hash::FxHashMap;
use std::sync::Arc;
use varn_core::AtomInterner;
use varn_tir::{Ancestry, BackendTy, ClassInfo, Signature, TyTable};

type ForeignFields = std::collections::BTreeMap<Arc<str>, Vec<crate::checker::InheritedField>>;

#[allow(clippy::too_many_arguments)]
pub(super) fn build_classes(
    bind: &BindResult,
    table: &CheckerTyTable,
    interner: &AtomInterner,
    tt: &mut TyTable,
    names: &NameIndex,
    class_names: &[Arc<str>],
    foreign: &ForeignFields,
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

#[allow(clippy::too_many_arguments)]
fn build_with_parents(
    bind: &BindResult,
    table: &CheckerTyTable,
    interner: &AtomInterner,
    tt: &mut TyTable,
    names: &NameIndex,
    class_names: &[Arc<str>],
    foreign: &ForeignFields,
    i: usize,
    signatures: &mut Vec<Signature>,
    built: &mut [Option<ClassInfo>],
) {
    let mut chain = vec![i];
    let mut cur = i;
    while let Some(p) = bind
        .class_parents
        .get(&class_names[cur])
        .and_then(|p| names.class_id(&p.name))
        .map(|id| id.0 as usize)
    {
        if built[p].is_some() || chain.contains(&p) {
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
                &class_names[c],
                foreign.get(&class_names[c]).map(Vec::as_slice),
                signatures,
                built,
            );
            built[c] = Some(info);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn build_one_class(
    bind: &BindResult,
    table: &CheckerTyTable,
    interner: &AtomInterner,
    tt: &mut TyTable,
    names: &NameIndex,
    name: &Arc<str>,
    foreign: Option<&[crate::checker::InheritedField]>,
    signatures: &mut Vec<Signature>,
    built: &[Option<ClassInfo>],
) -> ClassInfo {
    let parent_name = bind.class_parents.get(name);
    let parent_id = parent_name.and_then(|p| names.class_id(&p.name));
    let parent_info = parent_id.and_then(|id| built.get(id.0 as usize)?.as_ref());
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
                let sig = intern_signature(&m.ty, table, interner, tt, names, signatures);
                push_method(Arc::from(format!("get {}", m.name)), sig, &mut method_names);
            }
            ClassMemberKind::Setter => {
                let sig = intern_signature(&m.ty, table, interner, tt, names, signatures);
                push_method(Arc::from(format!("set {}", m.name)), sig, &mut method_names);
            }
            _ => {}
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
    let mut info = ClassInfo::new_with_methods(name.clone(), ancestry, fields, methods);
    info.constructor = constructor;
    info
}

fn field_backend_ty(
    ty: crate::types::Type,
    optional: bool,
    table: &CheckerTyTable,
    interner: &AtomInterner,
    tt: &mut TyTable,
    names: &NameIndex,
) -> BackendTy {
    let inner = lower_type(&ty, table, interner, tt, names);
    if optional && !matches!(inner, BackendTy::Nullable(_)) {
        BackendTy::Nullable(tt.intern(inner))
    } else {
        inner
    }
}
