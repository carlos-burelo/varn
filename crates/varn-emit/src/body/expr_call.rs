use super::call_contract::Callee;
use super::context::FnEmitter;
use varn_core::ast::{Arg, AstId, ExprId, ExprKind};
use varn_tir::{BackendTy, Resolution, Span, TirArg, TirExpr, TirExprKind, TirUnOp};

impl<'a> FnEmitter<'a> {
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
            let targs = self.settle_super_args(
                call_id,
                varn_core::TokenKind::Constructor.as_str(),
                targs,
                span,
            );
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
            if matches!(self.ast_arena.expr(object).kind, ExprKind::Super)
                && let Some(name) = Self::member_name(self.ast_arena, property, self.m.interner)
            {
                let targs = self.lower_call_args(call_id, args);
                let targs = self.settle_super_args(call_id, &name, targs, span);
                let reached = if self.has_own_this() {
                    None
                } else {
                    self.inherited_member(&name, false, span)
                };
                if let Some(callee) = reached {
                    return TirExpr {
                        kind: TirExprKind::Call {
                            callee: Box::new(callee),
                            args: targs,
                        },
                        ty,
                        res: Resolution::None,
                        span,
                    };
                }
                return TirExpr {
                    kind: TirExprKind::SuperMethodCall { name, args: targs },
                    ty,
                    res: Resolution::None,
                    span,
                };
            }
        }

        if let Some(variant) = self.bare_variant_call(call_id, callee, args, span) {
            return variant;
        }
        if let ExprKind::Identifier { name } = &self.ast_arena.expr(callee).kind {
            let c = self.lower_expr(callee);
            let targs = self.lower_call_args(call_id, args);
            let callee_sig = self.callee_signature(callee);
            let (c, targs) = if matches!(c.res, Resolution::NativeGlobal(_)) {
                (c, self.guard_args(Some(call_id), targs, callee_sig))
            } else {
                let (target, targs) =
                    self.settle_args(Some(call_id), Callee::Value(c), targs, callee_sig, span);
                (target.into_expr(), targs)
            };
            let all_positional = targs.iter().all(|a| matches!(a, TirArg::Expr(_)));
            let takes_rest = callee_sig
                .and_then(|sig| self.signatures.get(sig.0 as usize))
                .is_none_or(|sig| sig.has_rest);
            let mut targs = targs;
            if let Some((_, arity)) = self
                .known_fn(*name)
                .filter(|_| all_positional && !takes_rest)
            {
                targs = self.fill_omitted(targs, Some(call_id), callee_sig);
                while targs.len() < arity as usize {
                    targs.push(TirArg::Expr(TirExpr {
                        kind: TirExprKind::NullLit,
                        ty: crate::ty::NULL_TY,
                        res: Resolution::None,
                        span: Span::EMPTY,
                    }));
                }
            }
            let res = match self.known_fn(*name) {
                Some((fn_id, arity))
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

        if let Some(call) = self.lower_namespace_call(call_id, callee, args, ty, span) {
            return call;
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
            | ExprKind::ImportMeta(_)
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
            return super::enum_variant::make_variant(enum_id, tag, vargs, span);
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
}
