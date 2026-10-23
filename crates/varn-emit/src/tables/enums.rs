use super::signatures::accessor_signature;
use super::{intern_signature, NameIndex};
use crate::ty::lower_type;
use std::sync::Arc;
use varn_core::AtomInterner;
use varn_sem::bind::BindResult;
use varn_sem::types::{CheckerTyTable, ClassMemberKind};
use varn_tir::{EnumInfo, Signature, TyTable, VariantInfo, VtableEntry};

pub(super) fn build_enums(
    bind: &BindResult,
    table: &CheckerTyTable,
    interner: &AtomInterner,
    tt: &mut TyTable,
    names: &NameIndex,
    enum_names: &[Arc<str>],
    signatures: &mut Vec<Signature>,
) -> Vec<EnumInfo> {
    enum_names
        .iter()
        .map(|name| EnumInfo {
            name: name.clone(),
            binding: Some(name.clone()),
            members: enum_members(bind, name, table, interner, tt, names, signatures),
            variants: bind
                .enum_layout(name)
                .unwrap_or_default()
                .into_iter()
                .enumerate()
                .map(|(tag, (variant, fields))| VariantInfo {
                    name: variant,
                    tag: tag as u16,
                    payload: fields
                        .iter()
                        .map(|(_, ty)| lower_type(ty, table, interner, tt, names))
                        .collect(),
                })
                .collect(),
        })
        .collect()
}

fn enum_members(
    bind: &BindResult,
    name: &Arc<str>,
    table: &CheckerTyTable,
    interner: &AtomInterner,
    tt: &mut TyTable,
    names: &NameIndex,
    signatures: &mut Vec<Signature>,
) -> Vec<VtableEntry> {
    use varn_core::AccessorKind;
    let members = bind
        .type_members
        .enums
        .get(name)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let mut out: Vec<VtableEntry> = Vec::new();
    for m in members.iter().filter(|m| !m.is_static) {
        let (key, sig): (Arc<str>, _) = match m.kind {
            ClassMemberKind::Method | ClassMemberKind::Function => (
                m.name.clone(),
                intern_signature(&m.ty, table, interner, tt, names, signatures),
            ),
            ClassMemberKind::Getter => (
                Arc::from(AccessorKind::Get.key(&m.name)),
                accessor_signature(
                    AccessorKind::Get,
                    &m.ty,
                    table,
                    interner,
                    tt,
                    names,
                    signatures,
                ),
            ),
            ClassMemberKind::Setter => (
                Arc::from(AccessorKind::Set.key(&m.name)),
                accessor_signature(
                    AccessorKind::Set,
                    &m.ty,
                    table,
                    interner,
                    tt,
                    names,
                    signatures,
                ),
            ),
            ClassMemberKind::Property
            | ClassMemberKind::Variable
            | ClassMemberKind::Constructor
            | ClassMemberKind::Class
            | ClassMemberKind::Interface
            | ClassMemberKind::Namespace
            | ClassMemberKind::Enum
            | ClassMemberKind::Struct => continue,
        };
        match out.iter_mut().find(|e| e.name == key) {
            Some(entry) => entry.sig = sig,
            None => out.push(VtableEntry { name: key, sig }),
        }
    }
    out
}
