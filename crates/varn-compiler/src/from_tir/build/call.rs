use super::context::{Builder, InlineFrame, Result};
use crate::ssa::ir::{InstKind, Value};
use crate::OptError;
use varn_tir::BackendTy;
use varn_tir::TirStmt;

impl<'m> Builder<'m> {
    pub(super) fn lower_call(
        &mut self,
        callee: Value,
        args: &[varn_tir::TirArg],
        ty: BackendTy,
    ) -> Result<Value> {
        let mut vals: Vec<(Value, bool)> = Vec::with_capacity(args.len());
        let mut any_spread = false;
        for a in args {
            match a {
                varn_tir::TirArg::Expr(e) => vals.push((self.lower_expr(e)?, false)),
                varn_tir::TirArg::Spread(e) => {
                    any_spread = true;
                    vals.push((self.lower_expr(e)?, true));
                }
                varn_tir::TirArg::Named { value, .. } => {
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
        args: &[varn_tir::TirArg],
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
                .any(|a| matches!(a, varn_tir::TirArg::Spread(_)))
        {
            return Ok(None);
        }
        let argv = self.lower_args(args)?;
        Ok(Some(self.emit(InstKind::SelfCall { args: argv }, ty)))
    }

    pub(super) fn try_inline_direct_call(
        &mut self,
        f: varn_tir::FnId,
        args: &[varn_tir::TirArg],
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
            return self.try_inline_body_call(f, args, call_ty, force);
        }
        let TirStmt::Return(Some(ref ret_expr)) = tf.body[0] else {
            return self.try_inline_body_call(f, args, call_ty, force);
        };
        if args.len() != tf.params.len()
            || args
                .iter()
                .any(|a| matches!(a, varn_tir::TirArg::Spread(_)))
        {
            if force {
                return Err(crate::OptError::Unsupported(
                    "`@inline` cannot apply to spread or arity-mismatched calls",
                ));
            }
            return Ok(None);
        }
        if matches!(ret_expr.kind, varn_tir::TirExprKind::Closure { .. }) {
            if force {
                return Err(crate::OptError::Unsupported(
                    "`@inline` cannot apply to functions returning closures",
                ));
            }
            return Ok(None);
        }

        let mut argv = Vec::with_capacity(args.len());
        for (a, &pty) in args.iter().zip(&tf.params) {
            let e = match a {
                varn_tir::TirArg::Expr(e) => e,
                varn_tir::TirArg::Named { value, .. } => value,
                varn_tir::TirArg::Spread(_) => return Ok(None),
            };
            argv.push(self.lower_inlined_arg(e, pty)?);
        }

        self.inlining.push(InlineFrame {
            func: f,
            params: argv,
            this: None,
            body: None,
        });
        let res = self.lower_expr(ret_expr);
        self.inlining.pop();

        let v = res?;
        let coerced = self.coerce(v, call_ty);
        Ok(Some(coerced))
    }

    pub(super) fn lower_inlined_arg(
        &mut self,
        arg: &varn_tir::TirExpr,
        param: varn_tir::BackendTy,
    ) -> Result<Value> {
        let boxed = matches!(
            arg.ty,
            varn_tir::BackendTy::Nullable(_) | varn_tir::BackendTy::Dynamic(_)
        );
        let precise = match &arg.kind {
            varn_tir::TirExprKind::Cast { operand } if boxed => operand.as_ref(),
            varn_tir::TirExprKind::IntLit(_)
            | varn_tir::TirExprKind::FloatLit(_)
            | varn_tir::TirExprKind::BoolLit(_)
            | varn_tir::TirExprKind::StrLit(_)
            | varn_tir::TirExprKind::CharLit(_)
            | varn_tir::TirExprKind::NullLit
            | varn_tir::TirExprKind::Var
            | varn_tir::TirExprKind::Binary { .. }
            | varn_tir::TirExprKind::Unary { .. }
            | varn_tir::TirExprKind::Field { .. }
            | varn_tir::TirExprKind::Index { .. }
            | varn_tir::TirExprKind::Call { .. }
            | varn_tir::TirExprKind::MethodCall { .. }
            | varn_tir::TirExprKind::Assign { .. }
            | varn_tir::TirExprKind::ArrayLit(_)
            | varn_tir::TirExprKind::TupleLit(_)
            | varn_tir::TirExprKind::ObjectLit { .. }
            | varn_tir::TirExprKind::RecordLit { .. }
            | varn_tir::TirExprKind::Await { .. }
            | varn_tir::TirExprKind::Yield { .. }
            | varn_tir::TirExprKind::Discriminant { .. }
            | varn_tir::TirExprKind::VariantPayload { .. }
            | varn_tir::TirExprKind::TypeTest { .. }
            | varn_tir::TirExprKind::Cast { .. }
            | varn_tir::TirExprKind::Checked { .. }
            | varn_tir::TirExprKind::Closure { .. }
            | varn_tir::TirExprKind::New { .. }
            | varn_tir::TirExprKind::MakeVariant { .. }
            | varn_tir::TirExprKind::Select { .. }
            | varn_tir::TirExprKind::Seq { .. }
            | varn_tir::TirExprKind::ObjectKeys { .. }
            | varn_tir::TirExprKind::Length { .. }
            | varn_tir::TirExprKind::SuperCall { .. }
            | varn_tir::TirExprKind::SuperMethodCall { .. }
            | varn_tir::TirExprKind::DecimalLit(_)
            | varn_tir::TirExprKind::BigIntLit(_)
            | varn_tir::TirExprKind::RangeLit { .. }
            | varn_tir::TirExprKind::ObjectRest { .. }
            | varn_tir::TirExprKind::ExtensionCall { .. } => arg,
        };
        let v = self.lower_expr(precise)?;
        Ok(self.widen_exact(v, precise.ty, param))
    }

    pub(super) fn lower_args(&mut self, args: &[varn_tir::TirArg]) -> Result<Vec<Value>> {
        let mut out = Vec::with_capacity(args.len());
        for a in args {
            match a {
                varn_tir::TirArg::Expr(e) => out.push(self.lower_expr(e)?),
                varn_tir::TirArg::Named { value, .. } => out.push(self.lower_expr(value)?),
                varn_tir::TirArg::Spread(_) => {
                    return Err(OptError::Unsupported("from_tir: spread in this position"))
                }
            }
        }
        Ok(out)
    }
}
