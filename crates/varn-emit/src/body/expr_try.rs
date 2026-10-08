use super::context::FnEmitter;
use varn_core::ast::ExprId;
use varn_tir::{BackendTy, Resolution, Span, TirBinOp, TirExpr, TirExprKind, TirStmt, TirUnOp};

impl<'a> FnEmitter<'a> {
    pub(super) fn lower_try_expr(
        &mut self,
        expression: ExprId,
        ty: BackendTy,
        span: Span,
    ) -> TirExpr {
        let _ = span;
        let (table, interner) = (self.m.checker_table, self.m.interner);
        let sum = self
            .expr_table
            .get(&expression.index())
            .and_then(|entry| entry.ty.core_sum(table, |a| interner.try_resolve(a)))
            .map(|(sum, _)| sum);
        let inner = self.lower_expr(expression);
        let hoisted = self.hoist(inner);
        let span = hoisted.span;

        if let (Some(sum), BackendTy::Enum(eid)) = (sum, hoisted.ty.non_nullable(self.tt)) {
            let (fail, pass) = match sum {
                varn_core::CoreSum::Result => ("Err", "Ok"),
                varn_core::CoreSum::Option => ("None", "Some"),
            };
            let tags = self.m.enums.get(eid.0 as usize).and_then(|info| {
                let variant = |name: &str| info.variants.iter().find(|v| v.name.as_ref() == name);
                let (fail, pass) = (variant(fail)?, variant(pass)?);
                Some((fail.tag, pass.tag, pass.payload.first().copied()?))
            });
            if let Some((fail_tag, pass_tag, payload_ty)) = tags {
                return self.unwrap_or_propagate(hoisted, eid, fail_tag, pass_tag, payload_ty, ty);
            }
        }

        if matches!(hoisted.ty, BackendTy::Nullable(_)) {
            let null_ty = BackendTy::Nullable(self.tt.intern(BackendTy::Never));
            let null_expr = TirExpr {
                kind: TirExprKind::NullLit,
                ty: null_ty,
                res: Resolution::None,
                span,
            };
            let cond = TirExpr {
                kind: TirExprKind::Unary {
                    op: TirUnOp::IsNull,
                    operand: Box::new(hoisted.clone()),
                },
                ty: BackendTy::Bool,
                res: Resolution::None,
                span,
            };
            self.pending.push(TirStmt::If {
                cond,
                then_body: vec![TirStmt::Return(Some(null_expr))],
                else_body: vec![],
            });
            let non_null_ty = hoisted.ty.non_nullable(self.tt);
            return TirExpr {
                kind: TirExprKind::Cast {
                    operand: Box::new(hoisted),
                },
                ty: non_null_ty,
                res: Resolution::None,
                span,
            };
        }

        hoisted
    }

    fn unwrap_or_propagate(
        &mut self,
        hoisted: TirExpr,
        eid: varn_tir::EnumId,
        fail_tag: u16,
        pass_tag: u16,
        payload_ty: BackendTy,
        ty: BackendTy,
    ) -> TirExpr {
        let span = hoisted.span;
        let disc = TirExpr {
            kind: TirExprKind::Discriminant {
                value: Box::new(hoisted.clone()),
            },
            ty: BackendTy::Int,
            res: Resolution::None,
            span,
        };
        let cond = TirExpr {
            kind: TirExprKind::Binary {
                op: TirBinOp::Eq,
                lhs: Box::new(disc),
                rhs: Box::new(TirExpr {
                    kind: TirExprKind::IntLit(fail_tag as i64),
                    ty: BackendTy::Int,
                    res: Resolution::None,
                    span,
                }),
            },
            ty: BackendTy::Bool,
            res: Resolution::None,
            span,
        };
        self.pending.push(TirStmt::If {
            cond,
            then_body: vec![TirStmt::Return(Some(hoisted.clone()))],
            else_body: vec![],
        });
        let payload = TirExpr {
            kind: TirExprKind::VariantPayload {
                value: Box::new(hoisted),
                tag: pass_tag,
                field: 0,
            },
            ty: payload_ty,
            res: Resolution::EnumVariant {
                enum_id: eid,
                tag: pass_tag,
            },
            span,
        };
        self.cast_to(payload, ty)
    }
}
