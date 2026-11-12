use crate::error::PResult;
use crate::stream::TokenStream;
use vn_core::ast::{ExprId, ExprKind};

pub(super) fn parse_class_expr(s: &mut TokenStream) -> PResult<ExprId> {
    let decl = crate::parser::parse_class_decl(s, vec![], false)?;
    let full_range = decl.range;
    Ok(s.expr(
        full_range,
        ExprKind::ClassExpr {
            declaration: Box::new(decl),
        },
    ))
}
