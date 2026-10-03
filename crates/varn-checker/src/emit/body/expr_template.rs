use super::context::FnEmitter;
use std::sync::Arc;
use varn_core::ast::ExprId;
use varn_tir::{
    BackendTy, DynReason, Resolution, Span, TirArg, TirArrayEl, TirBinOp, TirExpr, TirExprKind,
};

impl<'a> FnEmitter<'a> {
    pub(super) fn lower_tagged_template(
        &mut self,
        tag: ExprId,
        template: ExprId,
        ty: BackendTy,
        span: Span,
    ) -> TirExpr {
        use varn_core::ast::{ExprKind, TemplatePart};
        let ExprKind::Template { parts } = &self.ast_arena.expr(template).kind else {
            return self.lower_expr(template);
        };

        let str_lit = |s: &str| TirExpr {
            kind: TirExprKind::StrLit(Arc::from(s)),
            ty: BackendTy::Str,
            res: Resolution::None,
            span,
        };
        let mut strings: Vec<TirArrayEl> = Vec::new();
        let mut values: Vec<TirArg> = Vec::new();
        let mut cur = String::new();
        for part in parts {
            match part {
                TemplatePart::Literal(s) => cur.push_str(s),
                TemplatePart::Interpolation(e) => {
                    strings.push(TirArrayEl::Expr(str_lit(&cur)));
                    cur.clear();
                    let v = self.lower_expr(*e);
                    values.push(TirArg::Expr(v));
                }
            }
        }
        strings.push(TirArrayEl::Expr(str_lit(&cur)));

        let strings_arr = TirExpr {
            kind: TirExprKind::ArrayLit(strings),
            ty: BackendTy::Array(self.tt.intern(BackendTy::Str)),
            res: Resolution::None,
            span,
        };
        let mut all_args = vec![TirArg::Expr(strings_arr)];
        all_args.extend(values);

        if let ExprKind::Member {
            object,
            property,
            computed: false,
            ..
        } = &self.ast_arena.expr(tag).kind
        {
            let (object, property) = (*object, *property);
            if let Some(name) = Self::member_name(self.ast_arena, property, self.m.interner) {
                let recv = self.lower_expr(object);
                return TirExpr {
                    kind: TirExprKind::MethodCall {
                        recv: Box::new(recv),
                        name,
                        args: all_args,
                    },
                    ty,
                    res: Resolution::None,
                    span,
                };
            }
        }
        let callee = self.lower_expr(tag);
        let res = match &self.ast_arena.expr(tag).kind {
            ExprKind::Identifier { name } => Resolution::ByName {
                name: Arc::from(self.m.interner.resolve(*name)),
                why: DynReason::Unannotated,
            },
            _ => Resolution::None,
        };
        TirExpr {
            kind: TirExprKind::Call {
                callee: Box::new(callee),
                args: all_args,
            },
            ty,
            res,
            span,
        }
    }
}

impl<'a> FnEmitter<'a> {
    pub(super) fn lower_template(
        &mut self,
        parts: &[varn_core::ast::TemplatePart],
        span: Span,
    ) -> TirExpr {
        use varn_core::ast::TemplatePart;
        let str_expr = |kind, span| TirExpr {
            kind,
            ty: BackendTy::Str,
            res: Resolution::None,
            span,
        };
        let mut acc: Option<TirExpr> = None;
        for part in parts {
            let piece = match part {
                TemplatePart::Literal(s) => {
                    str_expr(TirExprKind::StrLit(Arc::from(s.as_str())), span)
                }
                TemplatePart::Interpolation(e) => {
                    let le = self.lower_expr(*e);
                    if le.ty == BackendTy::Str {
                        le
                    } else {
                        str_expr(
                            TirExprKind::Cast {
                                operand: Box::new(le),
                            },
                            span,
                        )
                    }
                }
            };
            acc = Some(match acc {
                None => piece,
                Some(a) => str_expr(
                    TirExprKind::Binary {
                        op: TirBinOp::Add,
                        lhs: Box::new(a),
                        rhs: Box::new(piece),
                    },
                    span,
                ),
            });
        }
        acc.unwrap_or_else(|| str_expr(TirExprKind::StrLit(Arc::from("")), span))
    }
}
