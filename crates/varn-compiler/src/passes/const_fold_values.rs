use super::const_fold::is_constant_kind;
use crate::hir::{HirBinOp, HirUnOp};
use crate::ssa::ir::InstKind;
use varn_core::{add_int, mul_int, neg_int, pow_int, sub_int};
use varn_tir::{BackendTy, DynReason};

pub(super) fn fold_convert(conv: varn_core::ValueConv, operand: &InstKind) -> Option<InstKind> {
    use varn_core::ValueConv::*;
    match (conv, operand) {
        (IntToFloat, InstKind::ConstInt(n)) => Some(InstKind::ConstFloat(*n as f64)),
        (FloatToInt, InstKind::ConstFloat(f)) => {
            varn_core::float_to_int(*f).map(InstKind::ConstInt)
        }
        (BigIntToInt, InstKind::ConstBigInt(b)) => {
            let b: num_bigint::BigInt = b.parse().ok()?;
            i64::try_from(&b).ok().map(InstKind::ConstInt)
        }
        _ => None,
    }
}

pub(super) fn fold_unary(op: HirUnOp, operand: &InstKind, _ty: BackendTy) -> Option<InstKind> {
    match (op, operand) {
        (HirUnOp::Neg, InstKind::ConstInt(x)) => neg_int(*x).map(InstKind::ConstInt),
        (HirUnOp::Neg, InstKind::ConstFloat(x)) => Some(InstKind::ConstFloat(-x)),
        (HirUnOp::Not, InstKind::ConstBool(x)) => Some(InstKind::ConstBool(!x)),
        (HirUnOp::BitNot, InstKind::ConstInt(x)) => Some(InstKind::ConstInt(!x)),
        _ => None,
    }
}

pub(super) fn fold_binary(
    op: HirBinOp,
    lhs: &InstKind,
    rhs: &InstKind,
    _ty: BackendTy,
) -> Option<InstKind> {
    use HirBinOp::*;
    match (lhs, rhs) {
        (InstKind::ConstInt(x), InstKind::ConstInt(y)) => match op {
            Add => add_int(*x, *y).map(InstKind::ConstInt),
            Sub => sub_int(*x, *y).map(InstKind::ConstInt),
            Mul => mul_int(*x, *y).map(InstKind::ConstInt),
            Div => varn_core::div_int(*x, *y).ok().map(InstKind::ConstInt),
            Mod => varn_core::rem_int(*x, *y).ok().map(InstKind::ConstInt),

            Pow => {
                if *y >= 0 && *y <= 30 {
                    pow_int(*x, *y as u32).map(InstKind::ConstInt)
                } else {
                    None
                }
            }
            Eq => Some(InstKind::ConstBool(x == y)),
            Ne => Some(InstKind::ConstBool(x != y)),
            Lt => Some(InstKind::ConstBool(x < y)),
            Le => Some(InstKind::ConstBool(x <= y)),
            Gt => Some(InstKind::ConstBool(x > y)),
            Ge => Some(InstKind::ConstBool(x >= y)),
            BitAnd => Some(InstKind::ConstInt(x & y)),
            BitOr => Some(InstKind::ConstInt(x | y)),
            BitXor => Some(InstKind::ConstInt(x ^ y)),
            Shl => varn_core::shl_int(*x, *y).map(InstKind::ConstInt),
            Shr => varn_core::shr_int(*x, *y).map(InstKind::ConstInt),
            Ushr => varn_core::ushr_int(*x, *y).map(InstKind::ConstInt),
            Instanceof | In => None,
        },
        (InstKind::ConstFloat(x), InstKind::ConstFloat(y)) => match op {
            Add => Some(InstKind::ConstFloat(x + y)),
            Sub => Some(InstKind::ConstFloat(x - y)),
            Mul => Some(InstKind::ConstFloat(x * y)),
            Div => Some(InstKind::ConstFloat(x / y)),
            Mod => Some(InstKind::ConstFloat(x % y)),
            Pow => Some(InstKind::ConstFloat(x.powf(*y))),
            Eq => Some(InstKind::ConstBool(x == y)),
            Ne => Some(InstKind::ConstBool(x != y)),
            Lt => Some(InstKind::ConstBool(x < y)),
            Le => Some(InstKind::ConstBool(x <= y)),
            Gt => Some(InstKind::ConstBool(x > y)),
            Ge => Some(InstKind::ConstBool(x >= y)),
            BitAnd | BitOr | BitXor | Shl | Shr | Ushr | Instanceof | In => None,
        },
        (InstKind::ConstBool(x), InstKind::ConstBool(y)) => match op {
            Eq => Some(InstKind::ConstBool(x == y)),
            Ne => Some(InstKind::ConstBool(x != y)),
            Add | Sub | Mul | Div | Mod | Pow | Lt | Le | Gt | Ge | BitAnd | BitOr | BitXor
            | Shl | Shr | Ushr | Instanceof | In => None,
        },
        (InstKind::ConstStr(x), InstKind::ConstStr(y)) => match op {
            Eq => Some(InstKind::ConstBool(x == y)),
            Ne => Some(InstKind::ConstBool(x != y)),
            Add | Sub | Mul | Div | Mod | Pow | Lt | Le | Gt | Ge | BitAnd | BitOr | BitXor
            | Shl | Shr | Ushr | Instanceof | In => None,
        },
        (InstKind::ConstChar(x), InstKind::ConstChar(y)) => match op {
            Eq => Some(InstKind::ConstBool(x == y)),
            Ne => Some(InstKind::ConstBool(x != y)),
            Add | Sub | Mul | Div | Mod | Pow | Lt | Le | Gt | Ge | BitAnd | BitOr | BitXor
            | Shl | Shr | Ushr | Instanceof | In => None,
        },
        (InstKind::ConstDecimal(x), InstKind::ConstDecimal(y)) => match op {
            Add => Some(InstKind::ConstDecimal(x + y)),
            Sub => Some(InstKind::ConstDecimal(x - y)),
            Mul => Some(InstKind::ConstDecimal(x * y)),
            Div => varn_core::numeric_big::div_decimal(x, y)
                .ok()
                .map(InstKind::ConstDecimal),
            Eq => Some(InstKind::ConstBool(x == y)),
            Ne => Some(InstKind::ConstBool(x != y)),
            Lt => Some(InstKind::ConstBool(x < y)),
            Le => Some(InstKind::ConstBool(x <= y)),
            Gt => Some(InstKind::ConstBool(x > y)),
            Ge => Some(InstKind::ConstBool(x >= y)),
            Mod | Pow | BitAnd | BitOr | BitXor | Shl | Shr | Ushr | Instanceof | In => None,
        },
        (InstKind::ConstBigInt(x), InstKind::ConstBigInt(y)) => fold_bigint(op, x, y),
        (InstKind::ConstNull, InstKind::ConstNull) => match op {
            Eq => Some(InstKind::ConstBool(true)),
            Ne => Some(InstKind::ConstBool(false)),
            Add | Sub | Mul | Div | Mod | Pow | Lt | Le | Gt | Ge | BitAnd | BitOr | BitXor
            | Shl | Shr | Ushr | Instanceof | In => None,
        },
        (InstKind::ConstNull, other) | (other, InstKind::ConstNull) => {
            if is_constant_kind(other) {
                match op {
                    Eq => Some(InstKind::ConstBool(false)),
                    Ne => Some(InstKind::ConstBool(true)),
                    Add | Sub | Mul | Div | Mod | Pow | Lt | Le | Gt | Ge | BitAnd | BitOr
                    | BitXor | Shl | Shr | Ushr | Instanceof | In => None,
                }
            } else {
                None
            }
        }
        _ => None,
    }
}

pub(super) fn const_inst_ty(kind: &InstKind) -> Option<BackendTy> {
    match kind {
        InstKind::ConstInt(_) => Some(BackendTy::Int),
        InstKind::ConstFloat(_) => Some(BackendTy::Float),
        InstKind::ConstBool(_) => Some(BackendTy::Bool),
        InstKind::ConstStr(_) => Some(BackendTy::Str),

        InstKind::ConstChar(_) => Some(BackendTy::Char),
        InstKind::ConstDecimal(_) | InstKind::ConstBigInt(_) => {
            Some(BackendTy::Dynamic(DynReason::Boxed))
        }
        InstKind::ConstNull => Some(BackendTy::Dynamic(DynReason::Boxed)),
        InstKind::Binary { .. }
        | InstKind::Unary { .. }
        | InstKind::LoadGlobal(_)
        | InstKind::LoadGlobalIdx(_)
        | InstKind::LoadNativeGlobalIdx(_)
        | InstKind::LoadUpvalue(_)
        | InstKind::StoreGlobal { .. }
        | InstKind::StoreGlobalIdx { .. }
        | InstKind::StoreUpvalue { .. }
        | InstKind::Call { .. }
        | InstKind::AllocInstance { .. }
        | InstKind::SelfCall { .. }
        | InstKind::GetProperty { .. }
        | InstKind::GetFixedField { .. }
        | InstKind::GetIndex { .. }
        | InstKind::ArrayGetIndex { .. }
        | InstKind::BytesGetIndex { .. }
        | InstKind::MapGetIndex { .. }
        | InstKind::SetProperty { .. }
        | InstKind::SetFixedField { .. }
        | InstKind::SetIndex { .. }
        | InstKind::ArraySetIndex { .. }
        | InstKind::BytesSetIndex { .. }
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
        | InstKind::MakeEnumVariant { .. }
        | InstKind::Try { .. }
        | InstKind::PopTry
        | InstKind::CatchParam { .. }
        | InstKind::CloseUpvalues { .. }
        | InstKind::Dispose { .. }
        | InstKind::LoadModule { .. }
        | InstKind::StoreModuleSlot { .. }
        | InstKind::Await { .. }
        | InstKind::Spawn { .. }
        | InstKind::Yield { .. }
        | InstKind::IntrinsicCall { .. }
        | InstKind::CallNativeOp { .. }
        | InstKind::AssertNotNull { .. }
        | InstKind::GetPropertyMaybe { .. }
        | InstKind::ModuleSlot { .. }
        | InstKind::GetEnumTag { .. }
        | InstKind::IsArray { .. }
        | InstKind::StrLength { .. }
        | InstKind::ArrayLength { .. }
        | InstKind::BytesLength { .. }
        | InstKind::This
        | InstKind::Range { .. }
        | InstKind::ObjectKeys { .. }
        | InstKind::GetSuper { .. }
        | InstKind::SuperCall { .. }
        | InstKind::SuperMethodCall { .. }
        | InstKind::ExtensionCall { .. }
        | InstKind::CallSpread { .. }
        | InstKind::BuildArraySpread { .. }
        | InstKind::BuildObjectSpread { .. } => None,
    }
}

pub(super) fn fold_bigint(op: HirBinOp, x: &str, y: &str) -> Option<InstKind> {
    use HirBinOp::*;
    let (a, b): (num_bigint::BigInt, num_bigint::BigInt) = (x.parse().ok()?, y.parse().ok()?);
    let big = |v: num_bigint::BigInt| Some(InstKind::ConstBigInt(v.to_string().into()));
    match op {
        Add => big(a + b),
        Sub => big(a - b),
        Mul => big(a * b),
        Div => varn_core::numeric_big::div_big(&a, &b).ok().and_then(big),
        Mod => varn_core::numeric_big::rem_big(&a, &b).ok().and_then(big),
        Eq => Some(InstKind::ConstBool(a == b)),
        Ne => Some(InstKind::ConstBool(a != b)),
        Lt => Some(InstKind::ConstBool(a < b)),
        Le => Some(InstKind::ConstBool(a <= b)),
        Gt => Some(InstKind::ConstBool(a > b)),
        Ge => Some(InstKind::ConstBool(a >= b)),
        Pow | BitAnd | BitOr | BitXor | Shl | Shr | Ushr | Instanceof | In => None,
    }
}
