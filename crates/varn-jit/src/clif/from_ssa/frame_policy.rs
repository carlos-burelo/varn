use super::numeric;
use super::Ctx;
use varn_types::ssa::{SsaOp, SsaProto};

pub(super) fn may_push_frame(ctx: &Ctx<'_>, op: &SsaOp) -> bool {
    match op {
        SsaOp::Call { .. }
        | SsaOp::CallNativeOp { .. }
        | SsaOp::MethodCall { .. }
        | SsaOp::IterCall { .. }
        | SsaOp::SuperCall { .. }
        | SsaOp::SuperMethodCall { .. }
        | SsaOp::ExtensionCall { .. }
        | SsaOp::CallSpread { .. }
        | SsaOp::Dispose { .. }
        | SsaOp::GetProperty { .. }
        | SsaOp::SetProperty { .. }
        | SsaOp::GetIndex { .. }
        | SsaOp::SetIndex { .. }
        | SsaOp::GetPropertyMaybe { .. }
        | SsaOp::GetSymbol { .. }
        | SsaOp::BuildObjectSpread { .. }
        | SsaOp::Spawn { .. }
        | SsaOp::LoadModule { .. }
        | SsaOp::Await { .. }
        | SsaOp::Yield { .. } => true,
        SsaOp::SelfCall { .. } => ctx.frame.is_some(),
        SsaOp::ConstInt(_) | SsaOp::ConstFloat(_) | SsaOp::ConstBool(_) | SsaOp::ConstNull | SsaOp::ConstStr(_) | SsaOp::Binary { .. } | SsaOp::Unary { .. } | SsaOp::AllocInstance { .. } | SsaOp::LoadGlobalIdx(_) | SsaOp::ArrayGetIndex { .. } | SsaOp::ArraySetIndex { .. } | SsaOp::ConstChar(_) | SsaOp::ConstBigInt(_) | SsaOp::ConstDecimal(_) | SsaOp::MakeEnumVariant { .. } | SsaOp::IntrinsicCall { .. } | SsaOp::LoadNativeGlobalIdx(_) | SsaOp::Try { .. } | SsaOp::PopTry | SsaOp::CatchParam { .. } | SsaOp::StoreGlobalIdx { .. } | SsaOp::MakeClosure { .. } | SsaOp::LoadCaptured { .. } | SsaOp::StoreCaptured { .. } | SsaOp::LoadUpvalue(_) | SsaOp::StoreUpvalue { .. } | SsaOp::CloseUpvalues { .. } | SsaOp::Cast { .. } | SsaOp::Convert { .. } | SsaOp::IsNull { .. } | SsaOp::Typeof { .. } | SsaOp::ToString { .. } | SsaOp::IsArray { .. } | SsaOp::GetEnumTag { .. } | SsaOp::ObjectKeys { .. } | SsaOp::BuildStr { .. } | SsaOp::BuildArray { .. } | SsaOp::BuildMap { .. } | SsaOp::BuildObject { .. } | SsaOp::ArrayLength { .. } | SsaOp::StrLength { .. } | SsaOp::BytesLength { .. } | SsaOp::ArrayPush { .. } | SsaOp::This | SsaOp::GetFixedField { .. } | SsaOp::SetFixedField { .. } | SsaOp::MakeClass { .. } | SsaOp::DeclareLayout { .. } | SsaOp::DefineMethod { .. } | SsaOp::GetSuper { .. } | SsaOp::LoadGlobal(_) | SsaOp::StoreGlobal { .. } | SsaOp::BuildTuple { .. } | SsaOp::BuildArraySpread { .. } | SsaOp::ObjectMerge { .. } | SsaOp::ObjectRest { .. } | SsaOp::AssertNotNull { .. } | SsaOp::BindMethod { .. } | SsaOp::ArrayExtend { .. } | SsaOp::WrapSpread { .. } | SsaOp::Range { .. } | SsaOp::ModuleSlot { .. } | SsaOp::StoreModuleSlot { .. } => false,
    }
}

pub(super) fn needs_frame(ssa: &SsaProto, op: &SsaOp) -> bool {
    matches!(
        op,
        SsaOp::Call { .. }
            | SsaOp::AllocInstance { .. }
            | SsaOp::CallNativeOp { .. }
            | SsaOp::MethodCall { .. }
            | SsaOp::LoadGlobalIdx(_)
            | SsaOp::LoadNativeGlobalIdx(_)
            | SsaOp::StoreGlobalIdx { .. }
            | SsaOp::MakeClosure { .. }
            | SsaOp::LoadCaptured { .. }
            | SsaOp::StoreCaptured { .. }
            | SsaOp::LoadUpvalue(_)
            | SsaOp::StoreUpvalue { .. }
            | SsaOp::CloseUpvalues { .. }
            | SsaOp::Try { .. }
            | SsaOp::PopTry
            | SsaOp::CatchParam { .. }
            | SsaOp::MakeEnumVariant { .. }
            | SsaOp::IntrinsicCall { .. }
            | SsaOp::ArrayGetIndex { .. }
            | SsaOp::ArraySetIndex { .. }
            | SsaOp::LoadGlobal(_)
            | SsaOp::StoreGlobal { .. }
            | SsaOp::BuildTuple { .. }
            | SsaOp::BuildArraySpread { .. }
            | SsaOp::BuildObjectSpread { .. }
            | SsaOp::ObjectMerge { .. }
            | SsaOp::ObjectRest { .. }
            | SsaOp::GetPropertyMaybe { .. }
            | SsaOp::AssertNotNull { .. }
            | SsaOp::BindMethod { .. }
            | SsaOp::ArrayExtend { .. }
            | SsaOp::WrapSpread { .. }
            | SsaOp::Range { .. }
            | SsaOp::GetSymbol { .. }
            | SsaOp::IterCall { .. }
            | SsaOp::SuperCall { .. }
            | SsaOp::SuperMethodCall { .. }
            | SsaOp::ExtensionCall { .. }
            | SsaOp::CallSpread { .. }
            | SsaOp::LoadModule { .. }
            | SsaOp::ModuleSlot { .. }
            | SsaOp::StoreModuleSlot { .. }
            | SsaOp::Await { .. }
            | SsaOp::Spawn { .. }
            | SsaOp::Yield { .. }
            | SsaOp::Dispose { .. }
    ) || matches!(op, SsaOp::Convert { operand, conv }
        if !numeric::is_inline_convert(ssa, *operand, *conv))
}
