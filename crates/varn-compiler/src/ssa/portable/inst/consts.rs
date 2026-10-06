use super::super::Site;
use crate::ssa::ir::InstKind;
use varn_types::ssa::SsaOp;

pub(super) fn try_project(kind: &InstKind, site: &Site) -> Option<SsaOp> {
    match kind {
        InstKind::ConstInt(n) => Some(SsaOp::ConstInt(*n)),
        InstKind::ConstFloat(f) => Some(SsaOp::ConstFloat(*f)),
        InstKind::ConstBool(b) => Some(SsaOp::ConstBool(*b)),
        InstKind::ConstNull => Some(SsaOp::ConstNull),
        InstKind::ConstStr(s) => Some(SsaOp::ConstStr(s.as_ref().into())),
        InstKind::ConstChar(c) => Some(SsaOp::ConstChar(*c)),
        InstKind::ConstBigInt(digits) => Some(SsaOp::ConstBigInt(digits.as_ref().into())),
        InstKind::ConstDecimal(d) => Some(SsaOp::ConstDecimal(d.to_string().into())),
        InstKind::MakeEnumVariant { tag, meta } => Some(SsaOp::MakeEnumVariant {
            tag: *tag,
            meta: meta.as_ref().into(),
        }),
        InstKind::LoadGlobalIdx(slot) => Some(SsaOp::LoadGlobalIdx(*slot)),
        InstKind::LoadNativeGlobalIdx(slot) => Some(SsaOp::LoadNativeGlobalIdx(*slot)),
        InstKind::StoreGlobalIdx { slot, value } => Some(SsaOp::StoreGlobalIdx {
            slot: *slot,
            value: value.0,
        }),
        InstKind::LoadGlobal(name) => Some(SsaOp::LoadGlobal(name.as_ref().into())),
        InstKind::StoreGlobal { name, value } => Some(SsaOp::StoreGlobal {
            name: name.as_ref().into(),
            value: value.0,
        }),
        InstKind::LoadModule { source } => Some(SsaOp::LoadModule {
            source: source.as_ref().into(),
            own_ip: site.own_ip,
            live: site.resume_live.clone(),
        }),
        InstKind::ModuleSlot { object, slot } => Some(SsaOp::ModuleSlot {
            object: object.0,
            slot: *slot,
        }),
        InstKind::StoreModuleSlot { value, slot } => Some(SsaOp::StoreModuleSlot {
            slot: *slot,
            value: value.0,
        }),
        _ => None,
    }
}
