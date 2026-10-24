use varn_core::intrinsic_ops::math::MathOp;
use varn_types::register_meta::SlotKind;
use varn_types::ssa::{SsaBinOp, SsaOp, SsaProto, SsaUnOp};

pub(super) fn keeps_views(ssa: &SsaProto, op: &SsaOp) -> bool {
    match op {
        SsaOp::ConstInt(_)
        | SsaOp::ConstFloat(_)
        | SsaOp::ConstBool(_)
        | SsaOp::ConstNull
        | SsaOp::Cast { .. }
        | SsaOp::LoadCaptured { .. }
        | SsaOp::StoreCaptured { .. }
        | SsaOp::ArrayGetIndex { .. }
        | SsaOp::ArraySetIndex { .. }
        | SsaOp::BytesSetIndex { .. }
        | SsaOp::BytesGetIndex { .. }
        | SsaOp::GetFixedField { .. }
        | SsaOp::ArrayLength { .. }
        | SsaOp::StrLength { .. }
        | SsaOp::BytesLength { .. }
        | SsaOp::LoadGlobal(_)
        | SsaOp::StoreGlobal { .. }
        | SsaOp::AssertNotNull { .. }
        | SsaOp::ModuleSlot { .. } => true,

        SsaOp::IntrinsicCall { args, wire, .. } => {
            matches!(args.as_slice(), [arg] if ssa.value_ty(*arg) == SlotKind::Float)
                && [MathOp::Abs, MathOp::Sqrt, MathOp::Floor, MathOp::Ceil]
                    .iter()
                    .any(|op| op.wire() == *wire)
        }

        SsaOp::Binary { op, .. } => match op {
            SsaBinOp::IntAdd
            | SsaBinOp::IntSub
            | SsaBinOp::IntMul
            | SsaBinOp::IntEq
            | SsaBinOp::IntNe
            | SsaBinOp::IntLt
            | SsaBinOp::IntLe
            | SsaBinOp::IntGt
            | SsaBinOp::IntGe
            | SsaBinOp::IntAnd
            | SsaBinOp::IntOr
            | SsaBinOp::IntXor
            | SsaBinOp::IntShl
            | SsaBinOp::IntShr
            | SsaBinOp::IntUshr
            | SsaBinOp::FloatAdd
            | SsaBinOp::FloatSub
            | SsaBinOp::FloatMul
            | SsaBinOp::FloatDiv
            | SsaBinOp::FloatEq
            | SsaBinOp::FloatNe
            | SsaBinOp::FloatLt
            | SsaBinOp::FloatLe
            | SsaBinOp::FloatGt
            | SsaBinOp::FloatGe => true,
            SsaBinOp::StrEq | SsaBinOp::StrNe => true,
            SsaBinOp::IntDiv
            | SsaBinOp::IntMod
            | SsaBinOp::IntPow
            | SsaBinOp::FloatMod
            | SsaBinOp::FloatPow
            | SsaBinOp::StrConcat
            | SsaBinOp::Dyn(_) => false,
        },
        SsaOp::Unary { op, .. } => match op {
            SsaUnOp::NegInt | SsaUnOp::NegFloat | SsaUnOp::Not | SsaUnOp::BitNotInt => true,
            SsaUnOp::Dyn(_) => false,
        },
        SsaOp::Convert { operand, conv } => super::numeric::is_inline_convert(ssa, *operand, *conv),
        SsaOp::ConstStr(_)
        | SsaOp::SelfCall { .. }
        | SsaOp::Call { .. }
        | SsaOp::AllocInstance { .. }
        | SsaOp::LoadGlobalIdx(_)
        | SsaOp::ConstChar(_)
        | SsaOp::ConstBigInt(_)
        | SsaOp::ConstDecimal(_)
        | SsaOp::MakeEnumVariant { .. }
        | SsaOp::LoadNativeGlobalIdx(_)
        | SsaOp::Try { .. }
        | SsaOp::PopTry
        | SsaOp::CatchParam { .. }
        | SsaOp::StoreGlobalIdx { .. }
        | SsaOp::MakeClosure { .. }
        | SsaOp::LoadUpvalue(_)
        | SsaOp::StoreUpvalue { .. }
        | SsaOp::CloseUpvalues { .. }
        | SsaOp::IsNull { .. }
        | SsaOp::Typeof { .. }
        | SsaOp::ToString { .. }
        | SsaOp::IsArray { .. }
        | SsaOp::GetEnumTag { .. }
        | SsaOp::ObjectKeys { .. }
        | SsaOp::BuildStr { .. }
        | SsaOp::BuildArray { .. }
        | SsaOp::BuildMap { .. }
        | SsaOp::BuildObject { .. }
        | SsaOp::GetProperty { .. }
        | SsaOp::SetProperty { .. }
        | SsaOp::GetIndex { .. }
        | SsaOp::SetIndex { .. }
        | SsaOp::MethodCall { .. }
        | SsaOp::CallNativeOp { .. }
        | SsaOp::ArrayPush { .. }
        | SsaOp::This
        | SsaOp::SetFixedField { .. }
        | SsaOp::MakeClass { .. }
        | SsaOp::DeclareLayout { .. }
        | SsaOp::DefineMethod { .. }
        | SsaOp::GetSuper { .. }
        | SsaOp::BuildTuple { .. }
        | SsaOp::BuildArraySpread { .. }
        | SsaOp::BuildObjectSpread { .. }
        | SsaOp::ObjectMerge { .. }
        | SsaOp::ObjectRest { .. }
        | SsaOp::GetPropertyMaybe { .. }
        | SsaOp::BindMethod { .. }
        | SsaOp::ArrayExtend { .. }
        | SsaOp::WrapSpread { .. }
        | SsaOp::Range { .. }
        | SsaOp::SuperCall { .. }
        | SsaOp::SuperMethodCall { .. }
        | SsaOp::ExtensionCall { .. }
        | SsaOp::CallSpread { .. }
        | SsaOp::LoadModule { .. }
        | SsaOp::StoreModuleSlot { .. }
        | SsaOp::Await { .. }
        | SsaOp::Spawn { .. }
        | SsaOp::Yield { .. }
        | SsaOp::Dispose { .. } => false,
    }
}
