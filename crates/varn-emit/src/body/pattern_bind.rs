use super::context::FnEmitter;
use super::small_utils::int_lit;
use std::sync::Arc;
use varn_core::ast::Pattern;
use varn_tir::{
    BackendTy, DynReason, Resolution, Span, TirArg, TirBinOp, TirExpr, TirExprKind, TirStmt,
    TirUnOp,
};

impl<'a> FnEmitter<'a> {
    pub fn destructure_params(&mut self, params: &[varn_core::ast::Param]) -> Vec<TirStmt> {
        let mut out = Vec::new();
        for (i, p) in params.iter().enumerate() {
            if let Some(def) = p.default {
                let pvar = || TirExpr {
                    kind: TirExprKind::Var,
                    ty: BackendTy::Dynamic(DynReason::NotYetSupported),
                    res: Resolution::Param(i as u32),
                    span: Span::EMPTY,
                };
                let is_null = TirExpr {
                    kind: TirExprKind::Unary {
                        op: TirUnOp::IsNull,
                        operand: Box::new(pvar()),
                    },
                    ty: BackendTy::Bool,
                    res: Resolution::None,
                    span: Span::EMPTY,
                };
                let value = self.lower_expr(def);
                let assign = TirExpr {
                    kind: TirExprKind::Assign {
                        target: Box::new(pvar()),
                        value: Box::new(value),
                    },
                    ty: BackendTy::Dynamic(DynReason::NotYetSupported),
                    res: Resolution::None,
                    span: Span::EMPTY,
                };
                out.append(&mut self.pending);
                out.push(TirStmt::If {
                    cond: is_null,
                    then_body: vec![TirStmt::Expr(assign)],
                    else_body: vec![],
                });
            }

            if matches!(p.pattern, Pattern::Identifier { .. }) {
                continue;
            }
            let src = TirExpr {
                kind: TirExprKind::Var,
                ty: BackendTy::Dynamic(DynReason::NotYetSupported),
                res: Resolution::Param(i as u32),
                span: Span::EMPTY,
            };
            self.bind_pattern(&p.pattern, src, &mut out);
        }
        out
    }

    pub(super) fn bind_pattern(&mut self, pat: &Pattern, src: TirExpr, out: &mut Vec<TirStmt>) {
        match pat {
            Pattern::Identifier { name, .. } => {
                let local = self.bind_local(Arc::from(self.m.interner.resolve(*name)), src.ty);
                out.push(TirStmt::Let {
                    local,
                    ty: src.ty,
                    init: Some(src),
                });
            }
            Pattern::Object {
                properties, rest, ..
            } => {
                for prop in properties {
                    let field = self.field_access(
                        src.clone(),
                        Arc::from(self.m.interner.resolve(prop.key)),
                        BackendTy::Dynamic(DynReason::NotYetSupported),
                        src.span,
                    );
                    self.bind_pattern(&prop.value, field, out);
                }
                if let Some(rest_pat) = rest {
                    let skip: Vec<Arc<str>> = properties
                        .iter()
                        .map(|p| Arc::from(self.m.interner.resolve(p.key)))
                        .collect();
                    let rest_obj = TirExpr {
                        kind: TirExprKind::ObjectRest {
                            object: Box::new(src.clone()),
                            skip_keys: skip,
                        },
                        ty: BackendTy::Dynamic(DynReason::NotYetSupported),
                        res: Resolution::None,
                        span: src.span,
                    };
                    self.bind_pattern(rest_pat, rest_obj, out);
                }
            }
            Pattern::Array { elements, rest, .. } => {
                let elem_ty = match src.ty.non_nullable(self.tt) {
                    BackendTy::Array(e) => self.tt.get(e),
                    BackendTy::Int
                    | BackendTy::Float
                    | BackendTy::Bool
                    | BackendTy::Char
                    | BackendTy::Str
                    | BackendTy::Bytes
                    | BackendTy::Decimal
                    | BackendTy::BigInt
                    | BackendTy::Map(..)
                    | BackendTy::Set(_)
                    | BackendTy::Tuple(_)
                    | BackendTy::Class(_)
                    | BackendTy::Enum(_)
                    | BackendTy::Fn(_)
                    | BackendTy::Nullable(_)
                    | BackendTy::Void
                    | BackendTy::Never
                    | BackendTy::Dynamic(_) => BackendTy::Dynamic(DynReason::NotYetSupported),
                };
                for (i, slot) in elements.iter().enumerate() {
                    let Some(el) = slot else { continue };

                    let idx = TirExpr {
                        kind: TirExprKind::Index {
                            object: Box::new(src.clone()),
                            index: Box::new(int_lit(i as i64)),
                        },
                        ty: elem_ty,
                        res: Resolution::None,
                        span: src.span,
                    };
                    let read = if matches!(el.pattern, Pattern::Assignment { .. }) {
                        self.element_if_present(src.clone(), i, idx)
                    } else {
                        idx
                    };
                    self.bind_pattern(&el.pattern, read, out);
                }

                if let Some(rest_pat) = rest {
                    let tail = TirExpr {
                        kind: TirExprKind::MethodCall {
                            recv: Box::new(src.clone()),
                            name: Arc::from("slice"),
                            args: vec![TirArg::Expr(int_lit(elements.len() as i64))],
                        },
                        ty: src.ty,
                        res: Resolution::ByName {
                            name: Arc::from("slice"),
                            why: DynReason::NotYetSupported,
                        },
                        span: src.span,
                    };
                    self.bind_pattern(rest_pat, tail, out);
                }
            }
            Pattern::Assignment { left, right, .. } => {
                let def = self.lower_expr(*right);
                out.extend(std::mem::take(&mut self.pending));
                let target_ty = if def.ty == src.ty.non_nullable(self.tt) {
                    def.ty
                } else {
                    src.ty
                };
                let is_null = TirExpr {
                    kind: TirExprKind::Unary {
                        op: TirUnOp::IsNull,
                        operand: Box::new(src.clone()),
                    },
                    ty: BackendTy::Bool,
                    res: Resolution::None,
                    span: src.span,
                };
                let value = TirExpr {
                    kind: TirExprKind::Select {
                        cond: Box::new(is_null),
                        then_val: Box::new(self.cast_to(def, target_ty)),
                        else_val: Box::new(self.cast_to(src.clone(), target_ty)),
                    },
                    ty: target_ty,
                    res: Resolution::None,
                    span: src.span,
                };
                self.bind_pattern(left, value, out);
            }

            Pattern::Rest { argument, .. } => self.bind_pattern(argument, src, out),
        }
    }

    fn element_if_present(&mut self, arr: TirExpr, i: usize, elem: TirExpr) -> TirExpr {
        let span = arr.span;
        let read_ty = match elem.ty {
            BackendTy::Nullable(_) | BackendTy::Dynamic(_) => elem.ty,
            t @ BackendTy::Int
            | t @ BackendTy::Float
            | t @ BackendTy::Bool
            | t @ BackendTy::Char
            | t @ BackendTy::Str
            | t @ BackendTy::Bytes
            | t @ BackendTy::Decimal
            | t @ BackendTy::BigInt
            | t @ BackendTy::Array(_)
            | t @ BackendTy::Map(..)
            | t @ BackendTy::Set(_)
            | t @ BackendTy::Tuple(_)
            | t @ BackendTy::Class(_)
            | t @ BackendTy::Enum(_)
            | t @ BackendTy::Fn(_)
            | t @ BackendTy::Void
            | t @ BackendTy::Never => BackendTy::Nullable(self.tt.intern(t)),
        };
        let len = self.field_access(arr, Arc::from("length"), BackendTy::Int, span);
        let present = TirExpr {
            kind: TirExprKind::Binary {
                op: TirBinOp::Lt,
                lhs: Box::new(int_lit(i as i64)),
                rhs: Box::new(len),
            },
            ty: BackendTy::Bool,
            res: Resolution::None,
            span,
        };
        TirExpr {
            kind: TirExprKind::Select {
                cond: Box::new(present),
                then_val: Box::new(self.cast_to(elem, read_ty)),
                else_val: Box::new(TirExpr {
                    kind: TirExprKind::NullLit,
                    ty: read_ty,
                    res: Resolution::None,
                    span,
                }),
            },
            ty: read_ty,
            res: Resolution::None,
            span,
        }
    }
}
