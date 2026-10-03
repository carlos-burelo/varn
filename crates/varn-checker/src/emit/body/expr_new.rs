use super::context::FnEmitter;
use crate::emit::ty::NameResolver;
use std::sync::Arc;
use varn_core::ast::{Arg, AstId, ExprId, ExprKind};
use varn_tir::{BackendTy, DynReason, Resolution, Span, TirExpr, TirExprKind};

impl<'a> FnEmitter<'a> {
    pub(super) fn lower_new(
        &mut self,
        call_id: AstId,
        callee: ExprId,
        args: &[Arg],
        ty: BackendTy,
        span: Span,
    ) -> TirExpr {
        let class = match &self.ast_arena.expr(callee).kind {
            ExprKind::Identifier { name } => self.m.names.class_id(self.m.interner.resolve(*name)),

            ExprKind::Member {
                property,
                computed: false,
                ..
            } => Self::member_name(self.ast_arena, *property, self.m.interner)
                .and_then(|n| self.m.names.class_id(&n)),
            _ => None,
        };
        let targs = self.lower_call_args(call_id, args);
        match class {
            Some(class) => TirExpr {
                kind: TirExprKind::New { class, args: targs },
                ty,
                res: Resolution::None,
                span,
            },

            None => {
                let c = self.lower_expr(callee);
                TirExpr {
                    kind: TirExprKind::Call {
                        callee: Box::new(c),
                        args: targs,
                    },
                    ty,
                    res: Resolution::ByName {
                        name: Arc::from("<new>"),
                        why: DynReason::Unannotated,
                    },
                    span,
                }
            }
        }
    }

    pub(super) fn enum_variant(
        &self,
        object: ExprId,
        variant: &str,
    ) -> Option<(varn_tir::EnumId, u16)> {
        let ExprKind::Identifier { name } = &self.ast_arena.expr(object).kind else {
            return None;
        };
        let eid = self.m.names.enum_id(self.m.interner.resolve(*name))?;
        let info = self.m.enums.get(eid.0 as usize)?;
        let v = info.variants.iter().find(|v| v.name.as_ref() == variant)?;
        Some((eid, v.tag))
    }
}
