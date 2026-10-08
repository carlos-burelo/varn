use crate::hir::{HirBinOp, HirType, HirUnOp};
use varn_core::OpCode;
use varn_types::ssa::{DynBinOp, DynUnOp, SsaBinOp, SsaUnOp};

pub(super) fn project_bin(
    op: HirBinOp,
    lhs_ty: Option<HirType>,
    rhs_ty: Option<HirType>,
    ty: HirType,
) -> Option<SsaBinOp> {
    use DynBinOp as D;
    let int = ty == HirType::Int;
    Some(match crate::lower::binary_opcode(op, ty, lhs_ty, rhs_ty) {
        OpCode::AddInt => SsaBinOp::IntAdd,
        OpCode::SubInt => SsaBinOp::IntSub,
        OpCode::MulInt => SsaBinOp::IntMul,
        OpCode::DivInt => SsaBinOp::IntDiv,
        OpCode::ModInt => SsaBinOp::IntMod,
        OpCode::PowInt => SsaBinOp::IntPow,
        OpCode::EqInt => SsaBinOp::IntEq,
        OpCode::NeqInt => SsaBinOp::IntNe,
        OpCode::LtInt => SsaBinOp::IntLt,
        OpCode::LteInt => SsaBinOp::IntLe,
        OpCode::GtInt => SsaBinOp::IntGt,
        OpCode::GteInt => SsaBinOp::IntGe,
        OpCode::BitAnd if int => SsaBinOp::IntAnd,
        OpCode::BitOr if int => SsaBinOp::IntOr,
        OpCode::BitXor if int => SsaBinOp::IntXor,
        OpCode::Shl if int => SsaBinOp::IntShl,
        OpCode::Shr if int => SsaBinOp::IntShr,
        OpCode::Ushr if int => SsaBinOp::IntUshr,
        OpCode::AddFloat => SsaBinOp::FloatAdd,
        OpCode::SubFloat => SsaBinOp::FloatSub,
        OpCode::MulFloat => SsaBinOp::FloatMul,
        OpCode::DivFloat => SsaBinOp::FloatDiv,
        OpCode::ModFloat => SsaBinOp::FloatMod,
        OpCode::PowFloat => SsaBinOp::FloatPow,
        OpCode::EqFloat => SsaBinOp::FloatEq,
        OpCode::NeqFloat => SsaBinOp::FloatNe,
        OpCode::LtFloat => SsaBinOp::FloatLt,
        OpCode::LteFloat => SsaBinOp::FloatLe,
        OpCode::GtFloat => SsaBinOp::FloatGt,
        OpCode::GteFloat => SsaBinOp::FloatGe,
        OpCode::StrConcat => SsaBinOp::StrConcat,
        OpCode::Add => SsaBinOp::Dyn(D::Add),
        OpCode::Sub => SsaBinOp::Dyn(D::Sub),
        OpCode::Mul => SsaBinOp::Dyn(D::Mul),
        OpCode::Div => SsaBinOp::Dyn(D::Div),
        OpCode::Mod => SsaBinOp::Dyn(D::Mod),
        OpCode::Pow => SsaBinOp::Dyn(D::Pow),
        OpCode::Eq => SsaBinOp::Dyn(D::Eq),
        OpCode::Neq => SsaBinOp::Dyn(D::Ne),
        OpCode::Lt => SsaBinOp::Dyn(D::Lt),
        OpCode::Lte => SsaBinOp::Dyn(D::Le),
        OpCode::Gt => SsaBinOp::Dyn(D::Gt),
        OpCode::Gte => SsaBinOp::Dyn(D::Ge),
        OpCode::BitAnd => SsaBinOp::Dyn(D::BitAnd),
        OpCode::BitOr => SsaBinOp::Dyn(D::BitOr),
        OpCode::BitXor => SsaBinOp::Dyn(D::BitXor),
        OpCode::Shl => SsaBinOp::Dyn(D::Shl),
        OpCode::Shr => SsaBinOp::Dyn(D::Shr),
        OpCode::Ushr => SsaBinOp::Dyn(D::Ushr),
        OpCode::Instanceof => SsaBinOp::Dyn(D::Instanceof),
        OpCode::In => SsaBinOp::Dyn(D::In),
        OpCode::LoadConst
        | OpCode::LoadNull
        | OpCode::LoadTrue
        | OpCode::LoadFalse
        | OpCode::LoadInt
        | OpCode::Move
        | OpCode::LoadGlobal
        | OpCode::StoreGlobal
        | OpCode::DefineGlobal
        | OpCode::DefineGlobalIdx
        | OpCode::LoadGlobalIdx
        | OpCode::StoreGlobalIdx
        | OpCode::LoadUpvalue
        | OpCode::StoreUpvalue
        | OpCode::CloseUpvalue
        | OpCode::Negate
        | OpCode::Not
        | OpCode::ToString
        | OpCode::Jump
        | OpCode::JumpIfFalse
        | OpCode::JumpIfTrue
        | OpCode::Loop
        | OpCode::Call
        | OpCode::CallMethod
        | OpCode::InvokeVirtual
        | OpCode::CallSpread
        | OpCode::Return
        | OpCode::BuildArray
        | OpCode::BuildTuple
        | OpCode::BuildObject
        | OpCode::BuildObjectWithShape
        | OpCode::BuildRecord
        | OpCode::GetIndex
        | OpCode::SetIndex
        | OpCode::ObjectRest
        | OpCode::ObjectKeys
        | OpCode::ObjectMerge
        | OpCode::GetProperty
        | OpCode::GetPropertyMaybe
        | OpCode::SetProperty
        | OpCode::GetFixedField
        | OpCode::SetFixedField
        | OpCode::GetSuper
        | OpCode::GetSymbol
        | OpCode::MakeClosure
        | OpCode::MakeClass
        | OpCode::Inherit
        | OpCode::Method
        | OpCode::DefineStatic
        | OpCode::DefineGetter
        | OpCode::DefineSetter
        | OpCode::DefineStaticGetter
        | OpCode::DefineStaticSetter
        | OpCode::DeclareLayout
        | OpCode::AllocInstance
        | OpCode::BindMethod
        | OpCode::Typeof
        | OpCode::IsNull
        | OpCode::IsArray
        | OpCode::AssertNotNull
        | OpCode::StrLength
        | OpCode::StrSlice
        | OpCode::ArrayLength
        | OpCode::ArrayPush
        | OpCode::ArrayPop
        | OpCode::ArrayExtend
        | OpCode::WrapSpread
        | OpCode::MakeEnumVariant
        | OpCode::GetEnumTag
        | OpCode::Await
        | OpCode::Spawn
        | OpCode::Yield
        | OpCode::Try
        | OpCode::Throw
        | OpCode::PopTry
        | OpCode::LoadModule
        | OpCode::LoadModuleSlot
        | OpCode::StoreModuleSlot
        | OpCode::InvokeRuntimeStatic
        | OpCode::AddImm
        | OpCode::SubImm
        | OpCode::BuildStr
        | OpCode::LoadIntZero
        | OpCode::LoadIntOne
        | OpCode::LoadIntMinusOne
        | OpCode::Intrinsic
        | OpCode::LoadStaticFn
        | OpCode::CallSelf
        | OpCode::Nop
        | OpCode::ArrayGetIndex
        | OpCode::ArraySetIndex
        | OpCode::CallNativeOp
        | OpCode::IntrinsicDirect
        | OpCode::LoadNativeGlobalIdx
        | OpCode::BuildMap
        | OpCode::MapGetIndex
        | OpCode::MapSetIndex
        | OpCode::Convert
        | OpCode::BytesLength => return None,
    })
}

pub(super) fn project_un(op: HirUnOp, operand_ty: Option<HirType>) -> Option<SsaUnOp> {
    Some(match (op, operand_ty) {
        (HirUnOp::Neg, Some(HirType::Int)) => SsaUnOp::NegInt,
        (HirUnOp::Neg, Some(HirType::Float)) => SsaUnOp::NegFloat,
        (HirUnOp::Neg, _) => SsaUnOp::Dyn(DynUnOp::Neg),
        (HirUnOp::Not, Some(HirType::Bool)) => SsaUnOp::Not,
        (HirUnOp::Not, _) => SsaUnOp::Dyn(DynUnOp::Not),
        (HirUnOp::BitNot, Some(HirType::Int)) => SsaUnOp::BitNotInt,
        (HirUnOp::BitNot, _) => SsaUnOp::Dyn(DynUnOp::BitNot),

        (HirUnOp::Typeof, _) => return None,
    })
}
