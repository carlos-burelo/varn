use super::CallTypeCtx;
use varn_core::ast::operators::BinaryOp;
use varn_core::ast::ExprId;
use varn_sem::types::numeric_ops::numeric_binary_type;
use varn_sem::types::Type;

pub(super) fn infer_binary(
    c: &mut CallTypeCtx,
    left: ExprId,
    right: ExprId,
    op: BinaryOp,
) -> Option<Type> {
    let l = c.infer(left)?.apparent(c.table);
    let r = c.infer(right)?.apparent(c.table);

    match op {
        BinaryOp::Add => {
            if l == Type::Str || r == Type::Str {
                Some(Type::Str)
            } else {
                numeric_binary_type(&l, &r, c.table)
            }
        }
        BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod | BinaryOp::Pow => {
            numeric_binary_type(&l, &r, c.table)
        }
        BinaryOp::Lt
        | BinaryOp::Gt
        | BinaryOp::LtEq
        | BinaryOp::GtEq
        | BinaryOp::Eq
        | BinaryOp::NotEq
        | BinaryOp::In => Some(Type::Bool),
        BinaryOp::BitAnd
        | BinaryOp::BitOr
        | BinaryOp::BitXor
        | BinaryOp::Shl
        | BinaryOp::Shr
        | BinaryOp::UShr => (l.is_int() && r.is_int()).then_some(Type::Int),
    }
}
