use varn_core::OpCode;

use crate::hir::{HirBinOp, HirType};

/// The opcode a binary instruction is emitted as: `+` with an operand
/// statically typed `str` is concatenation — what `arith::add` would work out
/// at run time, one type test at a time — and anything else is
/// [`bin_opcode`] for its type. The one decision: the bytecode emitter and the
/// portable SSA both read it.
pub(crate) fn binary_opcode(
    op: HirBinOp,
    ty: HirType,
    lhs_ty: Option<HirType>,
    rhs_ty: Option<HirType>,
) -> OpCode {
    let str_operand = matches!(op, HirBinOp::Add)
        && (matches!(lhs_ty, Some(HirType::Str)) || matches!(rhs_ty, Some(HirType::Str)));
    if str_operand {
        OpCode::StrConcat
    } else {
        bin_opcode(op, ty)
    }
}

/// Pick the typed binary opcode for an operand type the SSA emitter proved.
fn bin_opcode(op: HirBinOp, ty: HirType) -> OpCode {
    use HirBinOp::*;
    match ty {
        HirType::Int => match op {
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
            And | Or => OpCode::Add,
        },
        HirType::Float => match op {
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
            And | Or => OpCode::Add,
        },
        _ => match op {
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
            And | Or => OpCode::Add,
        },
    }
}
