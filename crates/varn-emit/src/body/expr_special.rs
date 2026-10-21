use super::context::FnEmitter;
use std::sync::Arc;
use varn_core::ast::ExprId;
use varn_tir::{BackendTy, DynReason, Resolution, Span, TirBinOp, TirExpr, TirExprKind, TirUnOp};

impl<'a> FnEmitter<'a> {
    pub(super) fn lower_decimal_kind(&self, raw: varn_core::Atom) -> Option<TirExprKind> {
        let text: Arc<str> = Arc::from(self.m.interner.resolve(raw).trim_end_matches('d'));
        Some(TirExprKind::DecimalLit(text))
    }

    pub(super) fn lower_bigint_kind(&self, raw: varn_core::Atom) -> Option<TirExprKind> {
        let text = self.m.interner.resolve(raw).trim_end_matches('n');
        let n = varn_core::numeric_big::parse_bigint_literal(text).unwrap_or_default();
        Some(TirExprKind::BigIntLit(Arc::from(n.to_string())))
    }

    pub(super) fn lower_range(
        &mut self,
        start: ExprId,
        end: ExprId,
        inclusive: bool,
        ty: BackendTy,
        span: Span,
    ) -> TirExpr {
        let s = self.lower_expr(start);
        let en = self.lower_expr(end);
        TirExpr {
            kind: TirExprKind::RangeLit {
                start: Box::new(s),
                end: Box::new(en),
                inclusive,
            },
            ty,
            res: Resolution::None,
            span,
        }
    }

    pub(super) fn lower_is(
        &mut self,
        expression: ExprId,
        type_ann: &varn_core::ast::TypeNode,
        span: Span,
    ) -> TirExpr {
        let v = self.lower_expr(expression);
        let v = self.pin(v);
        match self.type_test(&v, type_ann, span) {
            Some(test) => test,
            None => self.cast_to(v, BackendTy::Bool),
        }
    }

    fn type_test(
        &mut self,
        v: &TirExpr,
        type_ann: &varn_core::ast::TypeNode,
        span: Span,
    ) -> Option<TirExpr> {
        use varn_core::RuntimeKind;
        let bool_ty = BackendTy::Bool;
        let tag = |name: &'static str| {
            let got = TirExpr {
                kind: TirExprKind::Unary {
                    op: TirUnOp::Typeof,
                    operand: Box::new(v.clone()),
                },
                ty: BackendTy::Str,
                res: Resolution::None,
                span,
            };
            let want = TirExpr {
                kind: TirExprKind::StrLit(Arc::from(name)),
                ty: BackendTy::Str,
                res: Resolution::None,
                span,
            };
            TirExpr {
                kind: TirExprKind::Binary {
                    op: TirBinOp::Eq,
                    lhs: Box::new(got),
                    rhs: Box::new(want),
                },
                ty: BackendTy::Bool,
                res: Resolution::None,
                span,
            }
        };
        match &type_ann.kind {
            varn_core::TypeKind::Named(n, _) | varn_core::TypeKind::Generic(n, _, _) => {
                let written = self.m.interner.resolve(*n);
                if let Some(class) = self.class_named(written) {
                    return Some(TirExpr {
                        kind: TirExprKind::TypeTest {
                            value: Box::new(v.clone()),
                            class,
                        },
                        ty: bool_ty,
                        res: Resolution::None,
                        span,
                    });
                }
                let res = self.resolve_name(written);
                if matches!(res, Resolution::GlobalSlot(_)) {
                    return Some(TirExpr {
                        kind: TirExprKind::Binary {
                            op: TirBinOp::Instanceof,
                            lhs: Box::new(v.clone()),
                            rhs: Box::new(TirExpr {
                                kind: TirExprKind::Var,
                                ty: BackendTy::Dynamic(DynReason::NotYetSupported),
                                res,
                                span,
                            }),
                        },
                        ty: bool_ty,
                        res: Resolution::None,
                        span,
                    });
                }
                self.builtin_tag(type_ann).map(tag)
            }
            varn_core::TypeKind::Union(members) => {
                let mut any = TirExpr {
                    kind: TirExprKind::BoolLit(false),
                    ty: bool_ty,
                    res: Resolution::None,
                    span,
                };
                for member in members {
                    let test = self.type_test(v, member, span)?;
                    any = TirExpr {
                        kind: TirExprKind::Select {
                            cond: Box::new(any),
                            then_val: Box::new(TirExpr {
                                kind: TirExprKind::BoolLit(true),
                                ty: bool_ty,
                                res: Resolution::None,
                                span,
                            }),
                            else_val: Box::new(test),
                        },
                        ty: bool_ty,
                        res: Resolution::None,
                        span,
                    };
                }
                Some(any)
            }
            varn_core::TypeKind::Array(_) => Some(tag(RuntimeKind::Array.name())),
            varn_core::TypeKind::Tuple(_) => Some(tag(RuntimeKind::Tuple.name())),
            varn_core::TypeKind::Fn(_) => Some(tag(RuntimeKind::Function.name())),
            _ => self.builtin_tag(type_ann).map(tag),
        }
    }

    fn builtin_tag(&self, type_ann: &varn_core::ast::TypeNode) -> Option<&'static str> {
        if let Some(name) = type_ann.kind.lang_name() {
            return Some(name);
        }
        if let varn_core::TypeKind::Named(n, _) = &type_ann.kind
            && let Some(primitive) = varn_core::LangPrimitive::from_str(self.m.interner.resolve(*n))
        {
            return Some(primitive.name());
        }
        let table = self.m.checker_table;
        self.m.annotation_types.get(&type_ann.id).and_then(|ty| {
            ty.builtin_of(table)
                .map(|b| b.runtime_kind().name())
                .or_else(|| table.get(ty.0).lang_name())
        })
    }
}
