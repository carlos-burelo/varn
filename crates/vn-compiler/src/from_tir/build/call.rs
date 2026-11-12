use super::context::{Builder, InlineFrame, Result};
use crate::ssa::ir::{InstKind, Value};
use crate::OptError;
use vn_tir::BackendTy;
use vn_tir::TirStmt;

fn creates_closure(e: &vn_tir::TirExpr) -> bool {
    matches!(e.kind, vn_tir::TirExprKind::Closure { .. })
        || crate::from_tir::tir_children::child_exprs(e)
            .into_iter()
            .any(creates_closure)
}

impl<'m> Builder<'m> {
    pub(super) fn lower_call(
        &mut self,
        callee: Value,
        args: &[vn_tir::TirArg],
        ty: BackendTy,
    ) -> Result<Value> {
        let mut vals: Vec<(Value, bool)> = Vec::with_capacity(args.len());
        let mut any_spread = false;
        for a in args {
            match a {
                vn_tir::TirArg::Expr(e) => vals.push((self.lower_expr(e)?, false)),
                vn_tir::TirArg::Spread(e) => {
                    any_spread = true;
                    vals.push((self.lower_expr(e)?, true));
                }
                vn_tir::TirArg::Named { value, .. } => {
                    vals.push((self.lower_expr(value)?, false))
                }
            }
        }
        if any_spread {
            Ok(self.emit(InstKind::CallSpread { callee, args: vals }, ty))
        } else {
            let plain = vals.into_iter().map(|(v, _)| v).collect();
            Ok(self.emit(
                InstKind::Call {
                    callee,
                    args: plain,
                },
                ty,
            ))
        }
    }

    pub(super) fn try_lower_self_call(
        &mut self,
        args: &[vn_tir::TirArg],
        ty: BackendTy,
    ) -> Result<Option<Value>> {
        let Some(f) = self.self_fn else {
            return Ok(None);
        };
        let Some(tf) = self.tir.function(f) else {
            return Ok(None);
        };
        let declared = tf.params.len();
        if tf.is_async
            || tf.is_generator
            || args.len() != declared
            || args
                .iter()
                .any(|a| matches!(a, vn_tir::TirArg::Spread(_)))
        {
            return Ok(None);
        }
        let argv = self.lower_args(args)?;
        Ok(Some(self.emit(InstKind::SelfCall { args: argv }, ty)))
    }

    pub(super) fn try_inline_direct_call(
        &mut self,
        f: vn_tir::FnId,
        args: &[vn_tir::TirArg],
        call_ty: BackendTy,
    ) -> Result<Option<Value>> {
        let force = self
            .tir
            .function(f)
            .map(|tf| tf.force_inline)
            .unwrap_or(false);
        if self.inlining.iter().any(|frame| frame.func == f) || Some(f) == self.self_fn {
            if force {
                return Err(crate::OptError::Unsupported(
                    "`@inline` cannot apply to recursive functions",
                ));
            }
            return Ok(None);
        }
        if self.inlining.len() >= 4 && !force {
            return Ok(None);
        }
        let Some(tf) = self.tir.function(f) else {
            return Ok(None);
        };
        if tf.is_async || tf.is_generator || tf.has_rest || tf.has_this {
            if force {
                return Err(crate::OptError::Unsupported(
                    "`@inline` cannot apply to async, generator, rest or this-bound functions",
                ));
            }
            return Ok(None);
        }
        if !tf.locals.is_empty() || tf.body.len() != 1 {
            return self.try_inline_body_call(f, args, call_ty, force, None);
        }
        let TirStmt::Return(Some(ref ret_expr)) = tf.body[0] else {
            return self.try_inline_body_call(f, args, call_ty, force, None);
        };
        if args.len() != tf.params.len()
            || args
                .iter()
                .any(|a| matches!(a, vn_tir::TirArg::Spread(_)))
        {
            if force {
                return Err(crate::OptError::Unsupported(
                    "`@inline` cannot apply to spread or arity-mismatched calls",
                ));
            }
            return Ok(None);
        }
        if creates_closure(ret_expr) {
            if force {
                return Err(crate::OptError::Unsupported(
                    "`@inline` cannot apply to functions that create closures",
                ));
            }
            return Ok(None);
        }

        let mut argv = Vec::with_capacity(args.len());
        for (a, &pty) in args.iter().zip(&tf.params) {
            let e = match a {
                vn_tir::TirArg::Expr(e) => e,
                vn_tir::TirArg::Named { value, .. } => value,
                vn_tir::TirArg::Spread(_) => return Ok(None),
            };
            argv.push(self.lower_inlined_arg(e, pty)?);
        }

        self.push_inline(InlineFrame {
            func: f,
            params: argv,
            this: None,
            body: None,
        });
        if let Some(offset) = super::stmt::stmt_offset(&tf.body[0]) {
            self.set_line_at(offset);
        }
        let res = self.lower_expr(ret_expr);
        self.pop_inline();

        let v = res?;
        let coerced = self.coerce(v, call_ty);
        Ok(Some(coerced))
    }

    pub(super) fn lower_inlined_arg(
        &mut self,
        arg: &vn_tir::TirExpr,
        param: vn_tir::BackendTy,
    ) -> Result<Value> {
        let boxed = matches!(
            arg.ty,
            vn_tir::BackendTy::Nullable(_) | vn_tir::BackendTy::Dynamic(_)
        );
        let precise = match &arg.kind {
            vn_tir::TirExprKind::Cast { operand } if boxed => operand.as_ref(),
            vn_tir::TirExprKind::IntLit(_)
            | vn_tir::TirExprKind::FloatLit(_)
            | vn_tir::TirExprKind::BoolLit(_)
            | vn_tir::TirExprKind::StrLit(_)
            | vn_tir::TirExprKind::CharLit(_)
            | vn_tir::TirExprKind::NullLit
            | vn_tir::TirExprKind::Var
            | vn_tir::TirExprKind::Binary { .. }
            | vn_tir::TirExprKind::Unary { .. }
            | vn_tir::TirExprKind::Field { .. }
            | vn_tir::TirExprKind::Index { .. }
            | vn_tir::TirExprKind::Call { .. }
            | vn_tir::TirExprKind::MethodCall { .. }
            | vn_tir::TirExprKind::Assign { .. }
            | vn_tir::TirExprKind::ArrayLit(_)
            | vn_tir::TirExprKind::TupleLit(_)
            | vn_tir::TirExprKind::ObjectLit { .. }
            | vn_tir::TirExprKind::Await { .. }
            | vn_tir::TirExprKind::Yield { .. }
            | vn_tir::TirExprKind::Discriminant { .. }
            | vn_tir::TirExprKind::VariantPayload { .. }
            | vn_tir::TirExprKind::TypeTest { .. }
            | vn_tir::TirExprKind::Cast { .. }
            | vn_tir::TirExprKind::Checked { .. }
            | vn_tir::TirExprKind::Closure { .. }
            | vn_tir::TirExprKind::New { .. }
            | vn_tir::TirExprKind::MakeVariant { .. }
            | vn_tir::TirExprKind::Select { .. }
            | vn_tir::TirExprKind::Seq { .. }
            | vn_tir::TirExprKind::ObjectKeys { .. }
            | vn_tir::TirExprKind::Length { .. }
            | vn_tir::TirExprKind::SuperCall { .. }
            | vn_tir::TirExprKind::SuperMethodCall { .. }
            | vn_tir::TirExprKind::DecimalLit(_)
            | vn_tir::TirExprKind::BigIntLit(_)
            | vn_tir::TirExprKind::RangeLit { .. }
            | vn_tir::TirExprKind::ObjectRest { .. }
            | vn_tir::TirExprKind::ExtensionCall { .. } => arg,
        };
        let v = self.lower_expr(precise)?;
        Ok(self.widen_exact(v, precise.ty, param))
    }

    pub(super) fn lower_args(&mut self, args: &[vn_tir::TirArg]) -> Result<Vec<Value>> {
        let mut out = Vec::with_capacity(args.len());
        for a in args {
            match a {
                vn_tir::TirArg::Expr(e) => out.push(self.lower_expr(e)?),
                vn_tir::TirArg::Named { value, .. } => out.push(self.lower_expr(value)?),
                vn_tir::TirArg::Spread(_) => {
                    return Err(OptError::Unsupported("from_tir: spread in this position"))
                }
            }
        }
        Ok(out)
    }
}
