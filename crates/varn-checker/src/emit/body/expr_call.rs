use super::context::FnEmitter;
use super::small_utils::{bin_op, int_lit};
use std::sync::Arc;
use varn_core::ast::{Arg, AstId, ExprId, ExprKind};
use varn_tir::{BackendTy, DynReason, Resolution, Span, TirArg, TirExpr, TirExprKind, TirUnOp};

impl<'a> FnEmitter<'a> {
    pub(super) fn lower_arg(&mut self, a: &Arg) -> TirArg {
        match a {
            Arg::Positional(e) => TirArg::Expr(self.lower_expr(*e)),
            Arg::Spread(e) => TirArg::Spread(self.lower_expr(*e)),
            Arg::Named { label, value } => TirArg::Named {
                label: Arc::from(label.as_str()),
                value: self.lower_expr(*value),
            },
        }
    }

    pub(super) fn lower_call_args(&mut self, call_id: AstId, args: &[Arg]) -> Vec<TirArg> {
        match self.m.call_mappings.get(&call_id).cloned() {
            Some(mapping) => mapping
                .iter()
                .map(|opt| match opt {
                    Some(i) => match &args[*i] {
                        Arg::Positional(e) | Arg::Named { value: e, .. } => {
                            TirArg::Expr(self.lower_expr(*e))
                        }
                        Arg::Spread(e) => TirArg::Spread(self.lower_expr(*e)),
                    },
                    None => TirArg::Expr(TirExpr {
                        kind: TirExprKind::NullLit,
                        ty: BackendTy::Dynamic(DynReason::NotYetSupported),
                        res: Resolution::None,
                        span: Span::EMPTY,
                    }),
                })
                .collect(),
            None => args.iter().map(|a| self.lower_arg(a)).collect(),
        }
    }

    pub(super) fn lower_call(
        &mut self,
        call_id: AstId,
        callee: ExprId,
        args: &[Arg],
        ty: BackendTy,
        span: Span,
    ) -> TirExpr {
        if matches!(self.ast_arena.expr(callee).kind, ExprKind::Super) {
            let targs = self.lower_call_args(call_id, args);
            return TirExpr {
                kind: TirExprKind::SuperCall { args: targs },
                ty,
                res: Resolution::None,
                span,
            };
        }

        if let ExprKind::Member {
            object,
            property,
            computed: false,
            ..
        } = &self.ast_arena.expr(callee).kind
        {
            let (object, property) = (*object, *property);
            if matches!(self.ast_arena.expr(object).kind, ExprKind::Super) {
                if let Some(name) = Self::member_name(self.ast_arena, property, self.m.interner) {
                    let targs = self.lower_call_args(call_id, args);
                    return TirExpr {
                        kind: TirExprKind::SuperMethodCall { name, args: targs },
                        ty,
                        res: Resolution::None,
                        span,
                    };
                }
            }
        }

        if let ExprKind::Identifier { name } = &self.ast_arena.expr(callee).kind {
            let c = self.lower_expr(callee);
            let targs = self.lower_call_args(call_id, args);
            let all_positional = targs.iter().all(|a| matches!(a, TirArg::Expr(_)));
            let res = match self.m.fns.get(name) {
                Some(&(fn_id, arity))
                    if all_positional
                        && arity as usize == targs.len()
                        && !self.m.decorated_fns.contains(name) =>
                {
                    Resolution::DirectFn(varn_tir::FnId(fn_id))
                }
                _ if all_positional && self.m.math_intrinsics.contains_key(name) => {
                    Resolution::Intrinsic(self.m.math_intrinsics[name] as u16)
                }
                _ => match c.res {
                    Resolution::NativeGlobal(idx) => Resolution::NativeGlobal(idx),
                    Resolution::None
                    | Resolution::Local(_)
                    | Resolution::Param(_)
                    | Resolution::Upvalue(_)
                    | Resolution::GlobalSlot(_)
                    | Resolution::ModuleSlot { .. }
                    | Resolution::FieldSlot(_)
                    | Resolution::StaticField(_)
                    | Resolution::VtableSlot(_)
                    | Resolution::DirectFn(_)
                    | Resolution::Intrinsic(_)
                    | Resolution::NativeOp(_)
                    | Resolution::EnumVariant { .. }
                    | Resolution::ByName { .. } => Resolution::None,
                },
            };
            return TirExpr {
                kind: TirExprKind::Call {
                    callee: Box::new(c),
                    args: targs,
                },
                ty,
                res,
                span,
            };
        }

        let (object, property) = match &self.ast_arena.expr(callee).kind {
            ExprKind::Member {
                object,
                property,
                computed: false,
                ..
            } => (*object, *property),
            ExprKind::IntLiteral { .. }
            | ExprKind::FloatLiteral { .. }
            | ExprKind::BigIntLiteral { .. }
            | ExprKind::DecimalLiteral { .. }
            | ExprKind::StrLiteral { .. }
            | ExprKind::CharLiteral { .. }
            | ExprKind::BoolLiteral { .. }
            | ExprKind::NullLiteral
            | ExprKind::RegexLiteral { .. }
            | ExprKind::Template { .. }
            | ExprKind::TaggedTemplate { .. }
            | ExprKind::Identifier { .. }
            | ExprKind::Missing
            | ExprKind::This
            | ExprKind::Super
            | ExprKind::Array { .. }
            | ExprKind::Object { .. }
            | ExprKind::Tuple { .. }
            | ExprKind::Record { .. }
            | ExprKind::Unary { .. }
            | ExprKind::Update { .. }
            | ExprKind::Binary { .. }
            | ExprKind::Logical { .. }
            | ExprKind::Assign { .. }
            | ExprKind::Conditional { .. }
            | ExprKind::Member { .. }
            | ExprKind::Call { .. }
            | ExprKind::New { .. }
            | ExprKind::Function { .. }
            | ExprKind::Arrow { .. }
            | ExprKind::Sequence { .. }
            | ExprKind::Paren { .. }
            | ExprKind::Await { .. }
            | ExprKind::Spawn { .. }
            | ExprKind::Yield { .. }
            | ExprKind::Spread { .. }
            | ExprKind::Pipeline { .. }
            | ExprKind::Range { .. }
            | ExprKind::NonNull { .. }
            | ExprKind::Try { .. }
            | ExprKind::As { .. }
            | ExprKind::Satisfies { .. }
            | ExprKind::ClassExpr { .. }
            | ExprKind::Match { .. }
            | ExprKind::Is { .. }
            | ExprKind::With { .. }
            | ExprKind::MetaAccess { .. } => {
                return self.by_name_call(call_id, callee, args, ty, span)
            }
        };
        let Some(name) = Self::member_name(self.ast_arena, property, self.m.interner) else {
            return self.by_name_call(call_id, callee, args, ty, span);
        };

        if let Some(mangled) = self.m.desugar.extension_calls.get(&span.start).cloned() {
            let recv = self.lower_expr(object);
            let targs = self.lower_call_args(call_id, args);
            return TirExpr {
                kind: TirExprKind::ExtensionCall {
                    func: mangled,
                    recv: Box::new(recv),
                    args: targs,
                },
                ty,
                res: Resolution::None,
                span,
            };
        }

        if let Some((enum_id, tag)) = self.enum_variant(object, &name) {
            let vargs = self.lower_call_args(call_id, args);
            return TirExpr {
                kind: TirExprKind::MakeVariant { args: vargs },
                ty: BackendTy::Enum(enum_id),
                res: Resolution::EnumVariant { enum_id, tag },
                span,
            };
        }

        let recv = self.lower_expr(object);
        let targs = self.lower_call_args(call_id, args);

        let all_positional = targs.iter().all(|a| matches!(a, TirArg::Expr(_)));

        if all_positional {
            if let Some(cls) = self.core_class_name(recv.ty) {
                if self
                    .m
                    .core_ops
                    .contains(&(Arc::from(cls), Arc::clone(&name)))
                {
                    let op_id = varn_core::op_id::core_method_op_id(cls, &name);
                    return TirExpr {
                        kind: TirExprKind::MethodCall {
                            recv: Box::new(recv),
                            name,
                            args: targs,
                        },
                        ty,
                        res: Resolution::NativeOp(op_id),
                        span,
                    };
                }
            }
        }

        self.method_call(recv, name, targs, ty, span)
    }

    pub(super) fn method_call(
        &mut self,
        recv: TirExpr,
        name: Arc<str>,
        targs: Vec<TirArg>,
        ty: BackendTy,
        span: Span,
    ) -> TirExpr {
        let all_positional = targs.iter().all(|a| matches!(a, TirArg::Expr(_)));
        let res = self
            .class_of(recv.ty)
            .and_then(|ci| ci.method_slot(&name).map(|s| (s, ci)))
            .and_then(|(slot, ci)| {
                let entry = ci.method_at(slot)?;
                let sig = self.signatures.get(entry.sig.0 as usize)?;
                (all_positional && sig.params.len() == targs.len())
                    .then_some(Resolution::VtableSlot(slot))
            })
            .unwrap_or(Resolution::ByName {
                name: name.clone(),
                why: DynReason::NotYetSupported,
            });

        TirExpr {
            kind: TirExprKind::MethodCall {
                recv: Box::new(recv),
                name,
                args: targs,
            },
            ty,
            res,
            span,
        }
    }

    pub(super) fn lower_operator_call(
        &mut self,
        method: varn_core::capability::OperatorMethod,
        recv: ExprId,
        arg: Option<ExprId>,
        ty: BackendTy,
        span: Span,
    ) -> TirExpr {
        use varn_core::capability::OperatorShape;
        let recv = self.lower_expr(recv);
        let args = arg
            .map(|a| vec![TirArg::Expr(self.lower_expr(a))])
            .unwrap_or_default();
        let name: Arc<str> = Arc::from(method.method);
        match method.shape {
            OperatorShape::Value => self.method_call(recv, name, args, ty, span),
            OperatorShape::Equals => self.method_call(recv, name, args, BackendTy::Bool, span),
            OperatorShape::NotEquals => {
                let equals = self.method_call(recv, name, args, BackendTy::Bool, span);
                TirExpr {
                    kind: TirExprKind::Unary {
                        op: TirUnOp::Not,
                        operand: Box::new(equals),
                    },
                    ty: BackendTy::Bool,
                    res: Resolution::None,
                    span,
                }
            }
            OperatorShape::CompareToZero(op) => {
                let compared = self.method_call(recv, name, args, BackendTy::Int, span);
                TirExpr {
                    kind: TirExprKind::Binary {
                        op: bin_op(op).expect("comparison operators lower to a TirBinOp"),
                        lhs: Box::new(compared),
                        rhs: Box::new(int_lit(0)),
                    },
                    ty: BackendTy::Bool,
                    res: Resolution::None,
                    span,
                }
            }
        }
    }

    pub(super) fn lower_pipeline(
        &mut self,
        left: ExprId,
        right: ExprId,
        ty: BackendTy,
        span: Span,
    ) -> TirExpr {
        if let ExprKind::Call { callee, args, .. } = &self.ast_arena.expr(right).kind {
            let (callee, args) = (*callee, args);
            let interner = self.m.interner;
            let has_placeholder = args.iter().any(|a| {
                matches!(
                    a,
                    Arg::Positional(e) | Arg::Named { value: e, .. }
                        if matches!(&self.ast_arena.expr(*e).kind, ExprKind::Identifier { name } if interner.resolve(*name) == "_")
                )
            });
            if has_placeholder {
                let lv = self.lower_expr(left);
                let piped = self.hoist(lv);
                let c = self.lower_expr(callee);
                let targs: Vec<TirArg> = args
                    .iter()
                    .map(|a| match a {
                        Arg::Positional(e)
                        | Arg::Named { value: e, .. }
                            if matches!(&self.ast_arena.expr(*e).kind, ExprKind::Identifier { name } if interner.resolve(*name) == "_") =>
                        {
                            TirArg::Expr(piped.clone())
                        }
                        other @ Arg::Positional(_) | other @ Arg::Spread(_) | other @ Arg::Named { .. } => self.lower_arg(other),
                    })
                    .collect();
                return TirExpr {
                    kind: TirExprKind::Call {
                        callee: Box::new(c),
                        args: targs,
                    },
                    ty,
                    res: Resolution::None,
                    span,
                };
            }
        }
        let arg = self.lower_expr(left);
        let callee = self.lower_expr(right);
        TirExpr {
            kind: TirExprKind::Call {
                callee: Box::new(callee),
                args: vec![TirArg::Expr(arg)],
            },
            ty,
            res: Resolution::None,
            span,
        }
    }

    pub(super) fn by_name_call(
        &mut self,
        call_id: AstId,
        callee: ExprId,
        args: &[Arg],
        ty: BackendTy,
        span: Span,
    ) -> TirExpr {
        let c = self.lower_expr(callee);
        let targs = self.lower_call_args(call_id, args);
        TirExpr {
            kind: TirExprKind::Call {
                callee: Box::new(c),
                args: targs,
            },
            ty,
            res: Resolution::None,
            span,
        }
    }
}
