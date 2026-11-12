use super::context::FnEmitter;
use super::small_utils::prop_key_name;
use std::sync::Arc;
use vn_core::ast::{ArrayEl, ExprId, ObjectProp};
use vn_tir::{
    BackendTy, DynReason, Resolution, Span, TirArg, TirArrayEl, TirExpr, TirExprKind,
    TirObjectEntry,
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
                            BackendTy::Dynamic(DynReason::Callable),
                            None,
                            span,
                        );
                        entries.push(TirObjectEntry::Field {
                            name,
                            value: closure,
                        });
                    }
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

    fn lower_value_with(
        &mut self,
        class: vn_tir::ClassId,
        object: ExprId,
        properties: &[ObjectProp],
        ty: BackendTy,
        span: Span,
    ) -> TirExpr {
        let source = self.lower_expr(object);
        let source = self.pin(source);
        let mut given: Vec<(Arc<str>, TirExpr)> = Vec::new();
        for p in properties {
            if let ObjectProp::Property { key, value, .. } = p
                && let Some(name) = prop_key_name(key)
            {
                given.push((name, self.lower_expr(*value)));
            }
        }
        self.value_with_given(class, source, given, ty, span)
    }

    pub(super) fn value_with_given(
        &mut self,
        class: vn_tir::ClassId,
        source: TirExpr,
        mut given: Vec<(Arc<str>, TirExpr)>,
        ty: BackendTy,
        span: Span,
    ) -> TirExpr {
        let fields: Vec<(Arc<str>, BackendTy)> = self.m.classes[class.0 as usize]
            .fields
            .iter()
            .map(|f| (f.name.clone(), f.ty))
            .collect();
        let args = fields
            .into_iter()
            .map(|(name, field_ty)| {
                let value = match given.iter().position(|(given_name, _)| *given_name == name) {
                    Some(at) => given.swap_remove(at).1,
                    None => self.field_access(source.clone(), name, field_ty, span),
                };
                TirArg::Expr(value)
            })
            .collect();
        TirExpr {
            kind: TirExprKind::New { class, args },
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
        if let BackendTy::Class(class) = ty.non_nullable(self.tt)
            && self
                .m
                .classes
                .get(class.0 as usize)
                .is_some_and(|info| info.layout.is_value)
        {
            return self.lower_value_with(class, object, properties, ty, span);
        }
        let mut entries = vec![TirObjectEntry::Spread(self.lower_expr(object))];
        for p in properties {
            if let ObjectProp::Property { key, value, .. } = p
                && let Some(name) = prop_key_name(key)
            {
                entries.push(TirObjectEntry::Field {
                    name,
                    value: self.lower_expr(*value),
                });
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
