use super::context::FnEmitter;
use super::small_utils::prop_key_name;
use varn_core::ast::{ArrayEl, ExprId, ObjectProp};
use varn_tir::{
    BackendTy, DynReason, Resolution, Span, TirArrayEl, TirExpr, TirExprKind, TirObjectEntry,
};

impl<'a> FnEmitter<'a> {
    pub(super) fn lower_array(
        &mut self,
        elements: &[ArrayEl],
        ty: BackendTy,
        span: Span,
    ) -> TirExpr {
        let els = elements
            .iter()
            .map(|el| match el {
                ArrayEl::Expr(e) => TirArrayEl::Expr(self.lower_expr(*e)),
                ArrayEl::Spread(e) => TirArrayEl::Spread(self.lower_expr(*e)),
                ArrayEl::Hole => TirArrayEl::Hole,
            })
            .collect();
        TirExpr {
            kind: TirExprKind::ArrayLit(els),
            ty,
            res: Resolution::None,
            span,
        }
    }

    pub(super) fn lower_tuple(
        &mut self,
        elements: &[ExprId],
        ty: BackendTy,
        span: Span,
    ) -> TirExpr {
        let xs = elements.iter().map(|&e| self.lower_expr(e)).collect();
        TirExpr {
            kind: TirExprKind::TupleLit(xs),
            ty,
            res: Resolution::None,
            span,
        }
    }

    pub(super) fn lower_record(
        &mut self,
        properties: &[ObjectProp],
        ty: BackendTy,
        span: Span,
    ) -> TirExpr {
        let fields = properties
            .iter()
            .filter_map(|p| match p {
                ObjectProp::Property { key, value, .. } => {
                    Some((prop_key_name(key)?, self.lower_expr(*value)))
                }
                _ => None,
            })
            .collect();
        TirExpr {
            kind: TirExprKind::RecordLit { fields },
            ty,
            res: Resolution::None,
            span,
        }
    }

    pub(super) fn lower_object(
        &mut self,
        properties: &[ObjectProp],
        ty: BackendTy,
        span: Span,
    ) -> TirExpr {
        let mut entries: Vec<TirObjectEntry> = Vec::new();
        for p in properties {
            match p {
                ObjectProp::Property { key, value, .. } => {
                    if let Some(name) = prop_key_name(key) {
                        entries.push(TirObjectEntry::Field {
                            name,
                            value: self.lower_expr(*value),
                        });
                    }
                }
                ObjectProp::Spread { argument, .. } => {
                    entries.push(TirObjectEntry::Spread(self.lower_expr(*argument)));
                }

                ObjectProp::Method {
                    key,
                    params,
                    body,
                    is_async,
                    is_generator,
                    ..
                } => {
                    if let Some(name) = prop_key_name(key) {
                        let closure = self.lower_closure(
                            params,
                            super::expr_closure::ClosureBody::Stmt(*body),
                            *is_async,
                            *is_generator,
                            BackendTy::Dynamic(DynReason::NotYetSupported),
                            None,
                            span,
                        );
                        entries.push(TirObjectEntry::Field {
                            name,
                            value: closure,
                        });
                    }
                }

                _ => {}
            }
        }
        TirExpr {
            kind: TirExprKind::ObjectLit { entries },
            ty,
            res: Resolution::None,
            span,
        }
    }

    pub(super) fn lower_with(
        &mut self,
        object: ExprId,
        properties: &[ObjectProp],
        ty: BackendTy,
        span: Span,
    ) -> TirExpr {
        let mut entries = vec![TirObjectEntry::Spread(self.lower_expr(object))];
        for p in properties {
            if let ObjectProp::Property { key, value, .. } = p {
                if let Some(name) = prop_key_name(key) {
                    entries.push(TirObjectEntry::Field {
                        name,
                        value: self.lower_expr(*value),
                    });
                }
            }
        }
        TirExpr {
            kind: TirExprKind::ObjectLit { entries },
            ty,
            res: Resolution::None,
            span,
        }
    }
}
