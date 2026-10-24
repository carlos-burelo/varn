use super::signatures::accessor_signature;
use super::{intern_signature, NameIndex};
use crate::ty::lower_type;
use crate::ty::NameResolver;
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
            members: enum_members(
                bind.type_members
                    .enums
                    .get(name)
                    .into_iter()
                    .flatten()
                    .filter(|m| !m.is_static)
                    .map(|m| (m.kind, &m.name, &m.ty)),
                table,
                interner,
                tt,
                names,
                signatures,
            ),
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

pub(super) fn enum_members<'a>(
    members: impl Iterator<Item = (ClassMemberKind, &'a Arc<str>, &'a varn_sem::types::Type)>,
    table: &CheckerTyTable,
    interner: &AtomInterner,
    tt: &mut TyTable,
    names: &dyn NameResolver,
    signatures: &mut Vec<Signature>,
) -> Vec<VtableEntry> {
    use varn_core::AccessorKind;
    let mut out: Vec<VtableEntry> = Vec::new();
    for (kind, name, ty) in members {
        let (key, sig): (Arc<str>, _) = match kind {
            ClassMemberKind::Method | ClassMemberKind::Function => (
                name.clone(),
                intern_signature(ty, table, interner, tt, names, signatures),
            ),
            ClassMemberKind::Getter => (
                Arc::from(AccessorKind::Get.key(name)),
                accessor_signature(
                    AccessorKind::Get,
                    ty,
                    table,
                    interner,
                    tt,
                    names,
                    signatures,
                ),
            ),
            ClassMemberKind::Setter => (
                Arc::from(AccessorKind::Set.key(name)),
                accessor_signature(
                    AccessorKind::Set,
                    ty,
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
