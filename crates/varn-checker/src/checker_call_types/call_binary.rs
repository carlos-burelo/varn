use super::CallTypeCtx;
use varn_core::ast::operators::BinaryOp;
use varn_core::ast::ExprId;
use varn_sem::types::Type;

pub(super) fn infer_binary(
    c: &mut CallTypeCtx,
    left: ExprId,
    right: ExprId,
    op: BinaryOp,
) -> Option<Type> {
    let l = c.infer(left)?;
    let r = c.infer(right)?;

    match op {
        BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div => {
            if l == Type::Float || r == Type::Float {
                Some(Type::Float)
            } else {
                Some(Type::Int)
            }
        }
        BinaryOp::Lt | BinaryOp::Gt | BinaryOp::LtEq | BinaryOp::GtEq => Some(Type::Bool),
        BinaryOp::Mod
        | BinaryOp::Pow
        | BinaryOp::Eq
        | BinaryOp::NotEq
        | BinaryOp::BitAnd
        | BinaryOp::BitOr
        | BinaryOp::BitXor
        | BinaryOp::Shl
        | BinaryOp::Shr
        | BinaryOp::UShr
        | BinaryOp::Instanceof
        | BinaryOp::In => Some(Type::Dynamic),
    }
}
