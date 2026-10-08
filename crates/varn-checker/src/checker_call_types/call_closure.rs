use super::CallTypeCtx;
use crate::types::Type;
use varn_core::ast::expr::ArrowBody;
use varn_core::ast::{Param, TypeNode};

pub(super) fn infer_closure(
    c: &mut CallTypeCtx,
    params: &[Param],
    return_type: &Option<TypeNode>,
    body: Option<&ArrowBody>,
) -> Option<Type> {
    let ctx = c.ctx;
    match body {
        None => Some(crate::binder::build_fn_type(
            params,
            return_type,
            false,
            ctx,
            c.table,
            Type::Dynamic,
        )),
        Some(ArrowBody::Block(_)) => Some(crate::binder::build_fn_type(
            params,
            return_type,
            true,
            ctx,
            c.table,
            Type::Dynamic,
        )),
        Some(ArrowBody::Expr(e)) => {
            let inferred_ret = c.infer(*e).unwrap_or(Type::Dynamic);
            Some(crate::binder::build_fn_type(
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
