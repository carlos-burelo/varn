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
        InstKind::Binary { .. }
        | InstKind::Unary { .. }
        | InstKind::LoadUpvalue(_)
        | InstKind::StoreUpvalue { .. }
        | InstKind::Call { .. }
        | InstKind::AllocInstance { .. }
        | InstKind::SelfCall { .. }
        | InstKind::GetProperty { .. }
        | InstKind::GetFixedField { .. }
        | InstKind::GetIndex { .. }
        | InstKind::ArrayGetIndex { .. }
        | InstKind::MapGetIndex { .. }
        | InstKind::SetProperty { .. }
        | InstKind::SetFixedField { .. }
        | InstKind::SetIndex { .. }
        | InstKind::ArraySetIndex { .. }
        | InstKind::MapSetIndex { .. }
        | InstKind::ArrayPush { .. }
        | InstKind::ObjectMerge { .. }
        | InstKind::MethodCall { .. }
        | InstKind::IsNull { .. }
        | InstKind::Cast { .. }
        | InstKind::Convert { .. }
        | InstKind::BuildArray { .. }
        | InstKind::BuildTuple { .. }
        | InstKind::BuildObject { .. }
        | InstKind::BuildRecord { .. }
        | InstKind::BuildMap { .. }
        | InstKind::ObjectRest { .. }
        | InstKind::ToString { .. }
        | InstKind::BuildStr { .. }
        | InstKind::MakeClosure { .. }
        | InstKind::LoadCaptured { .. }
        | InstKind::StoreCaptured { .. }
        | InstKind::MakeClass { .. }
        | InstKind::DeclareLayout { .. }
        | InstKind::DefineStatic { .. }
        | InstKind::DefineMethod { .. }
        | InstKind::DefineAccessor { .. }
        | InstKind::Try { .. }
        | InstKind::PopTry
        | InstKind::CatchParam { .. }
        | InstKind::CloseUpvalues { .. }
        | InstKind::Dispose { .. }
        | InstKind::Await { .. }
        | InstKind::Spawn { .. }
        | InstKind::Yield { .. }
        | InstKind::IntrinsicCall { .. }
        | InstKind::CallNativeOp { .. }
        | InstKind::AssertNotNull { .. }
        | InstKind::GetPropertyMaybe { .. }
        | InstKind::GetEnumTag { .. }
        | InstKind::IsArray { .. }
        | InstKind::StrLength { .. }
        | InstKind::ArrayLength { .. }
        | InstKind::BytesLength { .. }
        | InstKind::This
        | InstKind::Range { .. }
        | InstKind::ObjectKeys { .. }
        | InstKind::GetSymbol { .. }
        | InstKind::IterCall { .. }
        | InstKind::GetSuper { .. }
        | InstKind::SuperCall { .. }
        | InstKind::SuperMethodCall { .. }
        | InstKind::ExtensionCall { .. }
        | InstKind::CallSpread { .. }
        | InstKind::BuildArraySpread { .. }
        | InstKind::BuildObjectSpread { .. } => None,
    }
}
