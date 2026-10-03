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
            if let Some(info) = self.m.enums.get(eid.0 as usize) {
                let variant = |name: &str| info.variants.iter().find(|v| v.name.as_ref() == name);
                let is_err_res = variant("Err").filter(|_| sum == varn_core::CoreSum::Result);
                let is_ok_res = variant("Ok").filter(|_| sum == varn_core::CoreSum::Result);
                if let (Some(err_var), Some(ok_var)) = (is_err_res, is_ok_res) {
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
                                kind: TirExprKind::IntLit(err_var.tag as i64),
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
                    return TirExpr {
                        kind: TirExprKind::VariantPayload {
                            value: Box::new(hoisted),
                            tag: ok_var.tag,
                            field: 0,
                        },
                        ty,
                        res: Resolution::EnumVariant {
                            enum_id: eid,
                            tag: ok_var.tag,
                        },
                        span,
                    };
                }

                let is_none_opt = variant("None").filter(|_| sum == varn_core::CoreSum::Option);
                let is_some_opt = variant("Some").filter(|_| sum == varn_core::CoreSum::Option);
                if let (Some(none_var), Some(some_var)) = (is_none_opt, is_some_opt) {
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
                                kind: TirExprKind::IntLit(none_var.tag as i64),
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
                    return TirExpr {
                        kind: TirExprKind::VariantPayload {
                            value: Box::new(hoisted),
                            tag: some_var.tag,
                            field: 0,
                        },
                        ty,
                        res: Resolution::EnumVariant {
                            enum_id: eid,
                            tag: some_var.tag,
                        },
                        span,
                    };
                }
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
}
