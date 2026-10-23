use crate::ssa::ir::InstKind;
use varn_tir::BackendTy;

pub(super) fn why_not(kind: &InstKind, value_tys: &[BackendTy]) -> String {
    let ty = |v: &crate::ssa::ir::Value| value_tys.get(v.0 as usize).copied();
    match kind {
        InstKind::Binary { op, lhs, rhs, .. } => {
            format!(
                "no portable form for {op:?} on {:?} and {:?}",
                ty(lhs),
                ty(rhs)
            )
        }
        InstKind::Unary { op, operand, .. } => {
            format!("no portable form for {op:?} on {:?}", ty(operand))
        }
        other @ InstKind::ConstInt(_)
        | other @ InstKind::ConstFloat(_)
        | other @ InstKind::ConstBool(_)
        | other @ InstKind::ConstStr(_)
        | other @ InstKind::ConstChar(_)
        | other @ InstKind::ConstDecimal(_)
        | other @ InstKind::ConstBigInt(_)
        | other @ InstKind::ConstNull
        | other @ InstKind::LoadGlobal(_)
        | other @ InstKind::LoadGlobalIdx(_)
        | other @ InstKind::LoadNativeGlobalIdx(_)
        | other @ InstKind::LoadUpvalue(_)
        | other @ InstKind::StoreGlobal { .. }
        | other @ InstKind::StoreGlobalIdx { .. }
        | other @ InstKind::StoreUpvalue { .. }
        | other @ InstKind::Call { .. }
        | other @ InstKind::AllocInstance { .. }
        | other @ InstKind::SelfCall { .. }
        | other @ InstKind::GetProperty { .. }
        | other @ InstKind::GetFixedField { .. }
        | other @ InstKind::GetIndex { .. }
        | other @ InstKind::ArrayGetIndex { .. }
        | other @ InstKind::BytesGetIndex { .. }
        | other @ InstKind::MapGetIndex { .. }
        | other @ InstKind::SetProperty { .. }
        | other @ InstKind::SetFixedField { .. }
        | other @ InstKind::SetIndex { .. }
        | other @ InstKind::ArraySetIndex { .. }
        | other @ InstKind::BytesSetIndex { .. }
        | other @ InstKind::MapSetIndex { .. }
        | other @ InstKind::ArrayPush { .. }
        | other @ InstKind::ObjectMerge { .. }
        | other @ InstKind::MethodCall { .. }
        | other @ InstKind::IsNull { .. }
        | other @ InstKind::Cast { .. }
        | other @ InstKind::Convert { .. }
        | other @ InstKind::BuildArray { .. }
        | other @ InstKind::BuildTuple { .. }
        | other @ InstKind::BuildObject { .. }
        | other @ InstKind::BuildRecord { .. }
        | other @ InstKind::BuildMap { .. }
        | other @ InstKind::ObjectRest { .. }
        | other @ InstKind::ToString { .. }
        | other @ InstKind::BuildStr { .. }
        | other @ InstKind::MakeClosure { .. }
        | other @ InstKind::LoadCaptured { .. }
        | other @ InstKind::StoreCaptured { .. }
        | other @ InstKind::MakeClass { .. }
        | other @ InstKind::DeclareLayout { .. }
        | other @ InstKind::DefineStatic { .. }
        | other @ InstKind::DefineMethod { .. }
        | other @ InstKind::DefineAccessor { .. }
        | other @ InstKind::MakeEnumVariant { .. }
        | other @ InstKind::Try { .. }
        | other @ InstKind::PopTry
        | other @ InstKind::CatchParam { .. }
        | other @ InstKind::CloseUpvalues { .. }
        | other @ InstKind::Dispose { .. }
        | other @ InstKind::LoadModule { .. }
        | other @ InstKind::StoreModuleSlot { .. }
        | other @ InstKind::Await { .. }
        | other @ InstKind::Spawn { .. }
        | other @ InstKind::Yield { .. }
        | other @ InstKind::IntrinsicCall { .. }
        | other @ InstKind::CallNativeOp { .. }
        | other @ InstKind::AssertNotNull { .. }
        | other @ InstKind::GetPropertyMaybe { .. }
        | other @ InstKind::ModuleSlot { .. }
        | other @ InstKind::GetEnumTag { .. }
        | other @ InstKind::IsArray { .. }
        | other @ InstKind::StrLength { .. }
        | other @ InstKind::ArrayLength { .. }
        | other @ InstKind::BytesLength { .. }
        | other @ InstKind::This
        | other @ InstKind::Range { .. }
        | other @ InstKind::ObjectKeys { .. }
        | other @ InstKind::GetSuper { .. }
        | other @ InstKind::SuperCall { .. }
        | other @ InstKind::SuperMethodCall { .. }
        | other @ InstKind::ExtensionCall { .. }
        | other @ InstKind::CallSpread { .. }
        | other @ InstKind::BuildArraySpread { .. }
        | other @ InstKind::BuildObjectSpread { .. } => {
            let full = format!("{other:?}");
            let end = full.find([' ', '(', '{']).unwrap_or(full.len());
            format!("no portable form for {}", &full[..end])
        }
    }
}
