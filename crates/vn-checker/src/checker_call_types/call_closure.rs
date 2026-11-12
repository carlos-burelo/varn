use super::CallTypeCtx;
use vn_core::ast::expr::ArrowBody;
use vn_core::ast::{Param, TypeNode};
use vn_sem::types::Type;

pub(super) fn infer_closure(
    c: &mut CallTypeCtx,
    params: &[Param],
    return_type: &Option<TypeNode>,
    body: Option<&ArrowBody>,
) -> Option<Type> {
    let ctx = c.ctx;
    match body {
        None => Some(vn_sem::infer::build_fn_type(
            params,
            return_type,
            false,
            ctx,
            c.table,
            Type::Dynamic,
        )),
        Some(ArrowBody::Block(_)) => Some(vn_sem::infer::build_fn_type(
            params,
            return_type,
            true,
            ctx,
            c.table,
            Type::Dynamic,
        )),
        Some(ArrowBody::Expr(e)) => {
            let inferred_ret = c.infer(*e).unwrap_or(Type::Dynamic);
            Some(vn_sem::infer::build_fn_type(
                params,
                return_type,
                true,
                ctx,
                c.table,
                inferred_ret,
            ))
        }
    }
}
