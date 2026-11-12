use super::CallTypeCtx;
use vn_core::ast::MatchCase;
use vn_sem::types::Type;

pub(super) fn infer_match(c: &mut CallTypeCtx, cases: &[MatchCase]) -> Option<Type> {
    let mut tys = Vec::new();
    for case in cases {
        match &case.body {
            vn_core::ast::MatchBody::Expr(e) => {
                if let Some(ty) = c.infer(*e) {
                    tys.push(ty);
                }
            }
            vn_core::ast::MatchBody::Block(_) => {
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
