use super::context::FnEmitter;
use crate::ty::NameResolver;
use std::sync::Arc;
use varn_core::ast::ExprId;
use varn_tir::{BackendTy, Resolution, Span, TirBinOp, TirExpr, TirExprKind, TirUnOp};

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
        let bool_ty = BackendTy::Bool;

        if let varn_core::TypeKind::Named(n, _) = &type_ann.kind {
            if let Some(class) = self.m.names.class_id(self.m.interner.resolve(*n)) {
                return TirExpr {
                    kind: TirExprKind::TypeTest {
                        value: Box::new(v),
                        class,
                    },
                    ty: bool_ty,
                    res: Resolution::None,
                    span,
                };
            }
        }

        let tag_name: Option<&'static str> = match &type_ann.kind {
            k @ (varn_core::TypeKind::Primitive(_)
            | varn_core::TypeKind::Builtin(_)
            | varn_core::TypeKind::Literal(_)) => k.lang_name(),
            varn_core::TypeKind::Named(n, _) => {
                varn_core::RuntimeKind::from_str(self.m.interner.resolve(*n)).map(|t| t.name())
            }
            varn_core::TypeKind::This
            | varn_core::TypeKind::Array(_)
            | varn_core::TypeKind::Union(_)
            | varn_core::TypeKind::Intersection(_)
            | varn_core::TypeKind::Tuple(_)
            | varn_core::TypeKind::Generic(..)
            | varn_core::TypeKind::TemplateLiteral(_)
            | varn_core::TypeKind::Fn(_)
            | varn_core::TypeKind::Object(_)
            | varn_core::TypeKind::Typeof(_)
            | varn_core::TypeKind::KeyOf(_)
            | varn_core::TypeKind::IndexedAccess { .. }
            | varn_core::TypeKind::Mapped { .. }
            | varn_core::TypeKind::Conditional { .. }
            | varn_core::TypeKind::Infer(_)
            | varn_core::TypeKind::EnumVariant { .. }
            | varn_core::TypeKind::TypePredicate { .. } => None,
        };
        if let Some(name) = tag_name {
            let got = TirExpr {
                kind: TirExprKind::Unary {
                    op: TirUnOp::Typeof,
                    operand: Box::new(v),
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
            return TirExpr {
                kind: TirExprKind::Binary {
                    op: TirBinOp::Eq,
                    lhs: Box::new(got),
                    rhs: Box::new(want),
                },
                ty: bool_ty,
                res: Resolution::None,
                span,
            };
        }

        self.cast_to(v, bool_ty)
    }
}
