use varn_core::OpCode;
use varn_tir::BackendTy;

use crate::hir::HirBinOp;

pub(crate) fn binary_opcode(
    op: HirBinOp,
    ty: BackendTy,
    lhs_ty: Option<BackendTy>,
    rhs_ty: Option<BackendTy>,
) -> OpCode {
    let str_operand = matches!(op, HirBinOp::Add)
        && (matches!(lhs_ty, Some(BackendTy::Str)) || matches!(rhs_ty, Some(BackendTy::Str)));
    if str_operand {
        OpCode::StrConcat
    } else {
        bin_opcode(op, ty)
    }
}

fn bin_opcode(op: HirBinOp, ty: BackendTy) -> OpCode {
    use HirBinOp::*;
    match ty {
        BackendTy::Int => match op {
            Add => OpCode::AddInt,
            Sub => OpCode::SubInt,
            Mul => OpCode::MulInt,
            Div => OpCode::DivInt,
            Mod => OpCode::ModInt,
            Pow => OpCode::PowInt,
            Eq => OpCode::EqInt,
            Ne => OpCode::NeqInt,
            Lt => OpCode::LtInt,
            Le => OpCode::LteInt,
            Gt => OpCode::GtInt,
            Ge => OpCode::GteInt,
            BitAnd => OpCode::BitAnd,
            BitOr => OpCode::BitOr,
            BitXor => OpCode::BitXor,
            Shl => OpCode::Shl,
            Shr => OpCode::Shr,
            Ushr => OpCode::Ushr,
            Instanceof => OpCode::Instanceof,
            In => OpCode::In,
        },
        BackendTy::Float => match op {
            Add => OpCode::AddFloat,
            Sub => OpCode::SubFloat,
            Mul => OpCode::MulFloat,
            Div => OpCode::DivFloat,
            Mod => OpCode::ModFloat,
            Pow => OpCode::PowFloat,
            Eq => OpCode::EqFloat,
            Ne => OpCode::NeqFloat,
            Lt => OpCode::LtFloat,
            Le => OpCode::LteFloat,
            Gt => OpCode::GtFloat,
            Ge => OpCode::GteFloat,
            BitAnd => OpCode::BitAnd,
            BitOr => OpCode::BitOr,
            BitXor => OpCode::BitXor,
            Shl => OpCode::Shl,
            Shr => OpCode::Shr,
            Ushr => OpCode::Ushr,
            Instanceof => OpCode::Instanceof,
            In => OpCode::In,
        },
        BackendTy::Str => match op {
            Eq => OpCode::EqStr,
            Ne => OpCode::NeqStr,
            Add => OpCode::Add,
            Sub => OpCode::Sub,
            Mul => OpCode::Mul,
            Div => OpCode::Div,
            Mod => OpCode::Mod,
            Pow => OpCode::Pow,
            Lt => OpCode::Lt,
            Le => OpCode::Lte,
            Gt => OpCode::Gt,
            Ge => OpCode::Gte,
            BitAnd => OpCode::BitAnd,
            BitOr => OpCode::BitOr,
            BitXor => OpCode::BitXor,
            Shl => OpCode::Shl,
            Shr => OpCode::Shr,
            Ushr => OpCode::Ushr,
            Instanceof => OpCode::Instanceof,
            In => OpCode::In,
        },
        BackendTy::Bool
        | BackendTy::Char
        | BackendTy::Bytes
        | BackendTy::Enum(_)
        | BackendTy::Fn(_)
        | BackendTy::Dynamic(_)
        | BackendTy::Decimal
        | BackendTy::BigInt
        | BackendTy::Void
        | BackendTy::Never
        | BackendTy::Array(_)
        | BackendTy::Map(..)
        | BackendTy::Dict(_)
        | BackendTy::Set(_)
        | BackendTy::Tuple(_)
        | BackendTy::Class(_)
        | BackendTy::Nullable(_) => match op {
            Add => OpCode::Add,
            Sub => OpCode::Sub,
            Mul => OpCode::Mul,
            Div => OpCode::Div,
            Mod => OpCode::Mod,
            Pow => OpCode::Pow,
            Eq => OpCode::Eq,
            Ne => OpCode::Neq,
            Lt => OpCode::Lt,
            Le => OpCode::Lte,
            Gt => OpCode::Gt,
            Ge => OpCode::Gte,
            BitAnd => OpCode::BitAnd,
            BitOr => OpCode::BitOr,
            BitXor => OpCode::BitXor,
            Shl => OpCode::Shl,
            Shr => OpCode::Shr,
            Ushr => OpCode::Ushr,
            Instanceof => OpCode::Instanceof,
            In => OpCode::In,
        },
    }
}
