use super::CallTypeCtx;
use crate::types::Type;
use varn_core::ast::MatchCase;

pub(super) fn infer_match(c: &mut CallTypeCtx, cases: &[MatchCase]) -> Option<Type> {
    let mut tys = Vec::new();
    for case in cases {
        match &case.body {
            varn_core::ast::MatchBody::Expr(e) => {
                if let Some(ty) = c.infer(*e) {
                    tys.push(ty);
                }
            }
            varn_core::ast::MatchBody::Block(_) => {
                tys.push(Type::Void);
            }
        }
    }
    if tys.is_empty() {
        Some(Type::Dynamic)
    } else {
        let first = tys[0];
        if tys.iter().all(|t| t == &first) {
            Some(first)
        } else {
            Some(Type::union(tys, c.table))
        }
    }
}
