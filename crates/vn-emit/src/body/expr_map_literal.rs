use super::context::FnEmitter;
use super::small_utils::int_lit;
use std::sync::Arc;
use vn_core::ast::expr::PropKey;
use vn_core::ast::{ExprId, ObjectProp};
use vn_tir::{BackendTy, DynReason, Resolution, Span, TirExpr, TirExprKind, TirStmt};

impl<'a> FnEmitter<'a> {
    pub(super) fn is_map_literal(&self, expr: ExprId) -> bool {
        self.m.desugar.map_literals.contains(&expr.index())
    }

    pub(super) fn lower_map_literal(
        &mut self,
        properties: &[ObjectProp],
        ty: BackendTy,
        span: Span,
    ) -> TirExpr {
        let Some(constructor) = vn_core::abi::native_global_index(vn_core::BuiltinType::Map.name())
        else {
            unreachable!()
        };
        let BackendTy::Map(key_ty, value_ty) = ty.non_nullable(self.tt) else {
            unreachable!()
        };
        let (key_ty, value_ty) = (self.tt.get(key_ty), self.tt.get(value_ty));
        let built = TirExpr {
            kind: TirExprKind::Call {
                callee: Box::new(TirExpr {
                    kind: TirExprKind::Var,
                    ty: BackendTy::Dynamic(DynReason::Runtime),
                    res: Resolution::NativeGlobal(constructor),
                    span,
                }),
                args: Vec::new(),
            },
            ty,
            res: Resolution::None,
            span,
        };
        let map = self.pin(built);
        for property in properties {
            let ObjectProp::Property { key, value, .. } = property else {
                continue;
            };
            let key = match key {
                PropKey::Identifier(name) | PropKey::Str(name) => TirExpr {
                    kind: TirExprKind::StrLit(Arc::from(name.as_str())),
                    ty: BackendTy::Str,
                    res: Resolution::None,
                    span,
                },
                PropKey::Int(n) => int_lit(*n),
                PropKey::Computed(expr) => self.lower_expr(*expr),
            };
            let key = self.widened(key, key_ty);
            let value = self.lower_expr(*value);
            let value = self.widened(value, value_ty);
            let target = TirExpr {
                kind: TirExprKind::Index {
                    object: Box::new(map.clone()),
                    index: Box::new(key),
                },
                ty: value_ty,
                res: Resolution::None,
                span,
            };
            let assign = TirExpr {
                kind: TirExprKind::Assign {
                    target: Box::new(target),
                    value: Box::new(value),
                },
                ty: value_ty,
                res: Resolution::None,
                span,
            };
            self.pending.push(TirStmt::Expr(assign));
        }
        map
    }
}
