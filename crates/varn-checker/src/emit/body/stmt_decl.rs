use super::context::FnEmitter;
use super::expr_closure::ClosureBody;
use super::match_pattern::MatchDest;
use super::small_utils::{int_lit, placeholder};
use crate::emit::ty::lower_type;
use std::sync::Arc;
use varn_core::ast::{ExprKind, Pattern};
use varn_tir::{
    BackendTy, DynReason, Resolution, Span, TirArg, TirBinOp, TirExpr, TirExprKind, TirStmt,
    TirUnOp,
};

impl<'a> FnEmitter<'a> {
    pub(super) fn lower_decl_stmt(&mut self, decl: &varn_core::ast::Decl) -> Vec<TirStmt> {
        use varn_core::ast::{Decl, ExportDecl};
        let unwrapped = match decl {
            Decl::Export(ExportDecl::Decl { declaration, .. }) => declaration.as_ref(),
            other => other,
        };

        if let Decl::Function(f) = unwrapped {
            let dyn_ty = BackendTy::Dynamic(DynReason::NotYetSupported);
            let local = self.bind_local(Arc::from(self.m.interner.resolve(f.id)), dyn_ty);
            let closure = self.lower_closure(
                &f.params,
                ClosureBody::Stmt(f.body),
                f.modifiers.is_async,
                f.modifiers.is_generator,
                dyn_ty,
                None,
                Span::EMPTY,
            );
            return vec![TirStmt::Let {
                local,
                ty: dyn_ty,
                init: Some(closure),
            }];
        }

        if let Decl::Namespace(ns) = unwrapped {
            let dyn_ty = BackendTy::Dynamic(DynReason::NotYetSupported);
            let local = self.bind_local(Arc::from(self.m.interner.resolve(ns.id)), dyn_ty);
            let mut entries: Vec<varn_tir::TirObjectEntry> = Vec::new();
            for m in &ns.body {
                let Decl::Export(ExportDecl::Decl { declaration, .. }) = m else {
                    continue;
                };
                if let Decl::Function(f) = declaration.as_ref() {
                    let closure = self.lower_closure(
                        &f.params,
                        ClosureBody::Stmt(f.body),
                        f.modifiers.is_async,
                        f.modifiers.is_generator,
                        dyn_ty,
                        None,
                        Span::EMPTY,
                    );
                    entries.push(varn_tir::TirObjectEntry::Field {
                        name: Arc::from(self.m.interner.resolve(f.id)),
                        value: closure,
                    });
                }
            }
            let obj = TirExpr {
                kind: TirExprKind::ObjectLit { entries },
                ty: dyn_ty,
                res: Resolution::None,
                span: Span::EMPTY,
            };
            return vec![TirStmt::Let {
                local,
                ty: dyn_ty,
                init: Some(obj),
            }];
        }
        let v = match decl {
            Decl::Variable(v) => v,
            Decl::Export(ExportDecl::Decl { declaration, .. }) => match declaration.as_ref() {
                Decl::Variable(v) => v,
                _ => return vec![],
            },
            _ => return vec![],
        };
        let mut out = Vec::new();
        for d in &v.declarators {
            match &d.id {
                Pattern::Identifier { name, .. } => {
                    let name_str = self.m.interner.resolve(*name);
                    if let Some(init) = d.init {
                        if let ExprKind::Match { subject, cases } = &self.ast_arena.expr(init).kind
                        {
                            let (subject, cases) = (*subject, cases);
                            let ty = self
                                .expr_table
                                .get(&init.index())
                                .map(|e| {
                                    let names = self.m.names;
                                    let table = self.m.checker_table;
                                    let interner = self.m.interner;
                                    lower_type(&e.ty, table, interner, self.tt, names)
                                })
                                .unwrap_or(BackendTy::Dynamic(DynReason::NotYetSupported));
                            let local = self.bind_local(Arc::from(name_str), ty);
                            out.push(TirStmt::Let {
                                local,
                                ty,
                                init: None,
                            });
                            out.extend(std::mem::take(&mut self.pending));
                            out.extend(self.lower_match(subject, cases, MatchDest::Assign(local)));
                            continue;
                        }
                    }

                    let prebound = {
                        let is_closure = d.init.is_some_and(|e| {
                            matches!(
                                self.ast_arena.expr(e).kind,
                                ExprKind::Arrow { .. } | ExprKind::Function { .. }
                            )
                        });
                        let is_global = self.top_level && self.m.globals.contains_key(name_str);
                        if is_closure && !is_global {
                            Some(self.bind_local(
                                Arc::from(name_str),
                                BackendTy::Dynamic(DynReason::NotYetSupported),
                            ))
                        } else {
                            None
                        }
                    };

                    let init = d.init.map(|e| self.lower_expr(e));

                    let ty = d
                        .type_ann
                        .as_ref()
                        .and_then(|t| self.m.annotation_types.get(&t.id))
                        .map(|resolved| {
                            lower_type(
                                resolved,
                                self.m.checker_table,
                                self.m.interner,
                                self.tt,
                                self.m.names,
                            )
                        })
                        .filter(|t| !matches!(t, BackendTy::Dynamic(_)))
                        .or_else(|| init.as_ref().map(|e| e.ty))
                        .unwrap_or(BackendTy::Dynamic(DynReason::NotYetSupported));
                    out.extend(std::mem::take(&mut self.pending));

                    if self.top_level && prebound.is_none() {
                        if let Some(&slot) = self.m.globals.get(name_str) {
                            let value = init.unwrap_or(TirExpr {
                                kind: TirExprKind::NullLit,
                                ty,
                                res: Resolution::None,
                                span: Span::EMPTY,
                            });
                            let target = TirExpr {
                                kind: TirExprKind::Var,
                                ty,
                                res: Resolution::GlobalSlot(slot),
                                span: Span::EMPTY,
                            };
                            out.push(TirStmt::Expr(TirExpr {
                                kind: TirExprKind::Assign {
                                    target: Box::new(target),
                                    value: Box::new(value),
                                },
                                ty: BackendTy::Void,
                                res: Resolution::None,
                                span: Span::EMPTY,
                            }));
                            continue;
                        }
                    }

                    let local =
                        prebound.unwrap_or_else(|| self.bind_local(Arc::from(name_str), ty));
                    out.push(TirStmt::Let { local, ty, init });
                }

                pat => {
                    let src = match d.init {
                        Some(init) => self.lower_expr(init),
                        None => placeholder(DynReason::NotYetSupported),
                    };
                    out.extend(std::mem::take(&mut self.pending));
                    let src = self.hoist(src);
                    out.extend(std::mem::take(&mut self.pending));
                    self.bind_pattern(pat, src, &mut out);
                }
            }
        }
        out
    }

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
                    _ => BackendTy::Dynamic(DynReason::NotYetSupported),
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
            t => BackendTy::Nullable(self.tt.intern(t)),
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
