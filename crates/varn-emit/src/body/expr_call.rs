use super::call_contract::Callee;
use super::context::FnEmitter;
use crate::ty::lower_type;
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
        let lowered = self.lower_args_in_order(call_id, args);
        let Some(checked) = self.m.desugar.call_params.get(&call_id) else {
            return lowered;
        };
        let (names, table, interner) = (self.m.names, self.m.checker_table, self.m.interner);
        let mut spread_seen = false;
        lowered
            .into_iter()
            .enumerate()
            .map(|(i, arg)| match arg {
                TirArg::Expr(e) => match checked.get(i) {
                    Some(param) if !spread_seen => {
                        let param = lower_type(&param.ty, table, interner, self.tt, names);
                        TirArg::Expr(self.widened(e, param))
                    }
                    Some(_) | None => TirArg::Expr(e),
                },
                spread @ TirArg::Spread(_) => {
                    spread_seen = true;
                    spread
                }
                named @ TirArg::Named { .. } => named,
            })
            .collect()
    }

    pub(super) fn checked_signature(
        &mut self,
        call_id: AstId,
        return_ty: BackendTy,
    ) -> Option<varn_tir::SigId> {
        let checked = self.m.desugar.call_params.get(&call_id)?;
        let (names, table, interner) = (self.m.names, self.m.checker_table, self.m.interner);
        let params = checked
            .iter()
            .map(|param| {
                let lowered = lower_type(&param.ty, table, interner, self.tt, names);
                let boxed = matches!(lowered, BackendTy::Nullable(_) | BackendTy::Dynamic(_));
                if param.optional && !boxed {
                    BackendTy::Nullable(self.tt.intern(lowered))
                } else {
                    lowered
                }
            })
            .collect();
        let sig = varn_tir::Signature {
            params,
            return_ty,
            has_rest: false,
        };
        let at = match self.signatures.iter().position(|known| *known == sig) {
            Some(at) => at,
            None => {
                self.signatures.push(sig);
                self.signatures.len() - 1
            }
        };
        Some(varn_tir::SigId(at as u32))
    }

    pub(super) fn widened(&self, arg: TirExpr, param: BackendTy) -> TirExpr {
        let target = param.non_nullable(self.tt);
        let wider = matches!(
            target,
            BackendTy::Float | BackendTy::Decimal | BackendTy::BigInt
        );
        if arg.ty == BackendTy::Int && wider {
            self.cast_to(arg, target)
        } else {
            arg
        }
    }

    fn lower_args_in_order(&mut self, call_id: AstId, args: &[Arg]) -> Vec<TirArg> {
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
            let targs = self.settle_super_args("constructor", targs, span);
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
                    let targs = self.settle_super_args(&name, targs, span);
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
            let callee_sig = self.callee_signature(callee);
            let (c, targs) = if matches!(c.res, Resolution::NativeGlobal(_)) {
                (c, self.guard_args(targs, callee_sig))
            } else {
                let (target, targs) = self.settle_args(Callee::Value(c), targs, callee_sig, span);
                (target.into_expr(), targs)
            };
            let all_positional = targs.iter().all(|a| matches!(a, TirArg::Expr(_)));
            let takes_rest = callee_sig
                .and_then(|sig| self.signatures.get(sig.0 as usize))
                .is_none_or(|sig| sig.has_rest);
            let mut targs = targs;
            if let Some(&(_, arity)) = self.m.fns.get(name).filter(|_| all_positional && !takes_rest)
            {
                targs = self.fill_omitted(targs, callee_sig);
                while targs.len() < arity as usize {
                    targs.push(TirArg::Expr(TirExpr {
                        kind: TirExprKind::NullLit,
                        ty: BackendTy::Dynamic(DynReason::NotYetSupported),
                        res: Resolution::None,
                        span: Span::EMPTY,
                    }));
                }
            }
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

        let (object, property, optional) = match &self.ast_arena.expr(callee).kind {
            ExprKind::Member {
                object,
                property,
                computed: false,
                optional,
            } => (*object, *property, *optional),
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
        if optional {
            let recv = self.pin(recv);
            let is_null = TirExpr {
                kind: TirExprKind::Unary {
                    op: TirUnOp::IsNull,
                    operand: Box::new(recv.clone()),
                },
                ty: BackendTy::Bool,
                res: Resolution::None,
                span,
            };
            let present = TirExpr {
                ty: recv.ty.non_nullable(self.tt),
                ..recv
            };
            let targs = self.lower_call_args(call_id, args);
            let ty = ty.non_nullable(self.tt);
            let checked = self.checked_signature(call_id, ty);
            let call = self.member_call(present, name, targs, ty, checked, span);
            return self.null_or(is_null, call, span);
        }
        let targs = self.lower_call_args(call_id, args);
        let checked = self.checked_signature(call_id, ty);
        self.member_call(recv, name, targs, ty, checked, span)
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
        let callee_sig = self.callee_signature(callee);
        let (target, targs) = self.settle_args(Callee::Value(c), targs, callee_sig, span);
        let c = target.into_expr();
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
