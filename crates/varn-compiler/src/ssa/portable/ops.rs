//! Operators projected onto the physical domain their operand types prove.

use crate::hir::{HirBinOp, HirType, HirUnOp};
use varn_core::OpCode;
use varn_types::ssa::{DynBinOp, DynUnOp, SsaBinOp, SsaUnOp};

/// The opcode the emitter selects ([`crate::lower::binary_opcode`]), mapped to
/// its portable mirror: a typed opcode to its native op, a generic one to the
/// same operator on boxed values. The bitwise opcodes serve every type, so
/// they are native only when the operands are `int`.
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
        _ => return None,
    })
}

/// A unary operator, native only on an operand of the type it is native for
/// (`!` on a `bool`, `-`/`~` on an `int`, `-` on a `float`); otherwise the
/// operator on the boxed value, as the bytecode's generic opcode runs it.
pub(super) fn project_un(op: HirUnOp, operand_ty: Option<HirType>) -> Option<SsaUnOp> {
    Some(match (op, operand_ty) {
        (HirUnOp::Neg, Some(HirType::Int)) => SsaUnOp::NegInt,
        (HirUnOp::Neg, Some(HirType::Float)) => SsaUnOp::NegFloat,
        (HirUnOp::Neg, _) => SsaUnOp::Dyn(DynUnOp::Neg),
        (HirUnOp::Not, Some(HirType::Bool)) => SsaUnOp::Not,
        (HirUnOp::Not, _) => SsaUnOp::Dyn(DynUnOp::Not),
        (HirUnOp::BitNot, Some(HirType::Int)) => SsaUnOp::BitNotInt,
        (HirUnOp::BitNot, _) => SsaUnOp::Dyn(DynUnOp::BitNot),
        // `typeof` has its own op (`SsaOp::Typeof`).
        (HirUnOp::Typeof, _) => return None,
    })
}
