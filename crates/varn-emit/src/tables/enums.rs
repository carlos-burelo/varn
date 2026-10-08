use super::NameIndex;
use crate::ty::lower_type;
use std::sync::Arc;
use varn_core::AtomInterner;
use varn_sem::bind::BindResult;
use varn_sem::types::CheckerTyTable;
use varn_tir::{EnumInfo, TyTable, VariantInfo};

pub(super) fn build_enums(
    bind: &BindResult,
    table: &CheckerTyTable,
    interner: &AtomInterner,
    tt: &mut TyTable,
    names: &NameIndex,
    enum_names: &[Arc<str>],
) -> Vec<EnumInfo> {
    enum_names
        .iter()
        .map(|name| EnumInfo {
            name: name.clone(),
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
