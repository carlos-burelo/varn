use super::context::{Builder, Result};
use crate::hir::HirType;
use crate::ssa::ir::{InstKind, Value};
use crate::OptError;
use varn_tir::TirStmt;

impl<'m> Builder<'m> {
    pub(super) fn lower_call(
        &mut self,
        callee: Value,
        args: &[varn_tir::TirArg],
        ty: HirType,
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
        ty: HirType,
    ) -> Result<Option<Value>> {
        if args
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
        call_ty: HirType,
    ) -> Result<Option<Value>> {
        if self.inlining_stack.len() >= 4 || self.inlining_stack.contains(&f) {
            return Ok(None);
        }
        if Some(f) == self.self_fn {
            return Ok(None);
        }
        let Some(tf) = self.tir.function(f) else {
            return Ok(None);
        };
        if tf.is_async || tf.is_generator || tf.has_rest || tf.has_this {
            return Ok(None);
        }
        if tf.body.len() != 1 {
            return Ok(None);
        }
        let TirStmt::Return(Some(ref ret_expr)) = tf.body[0] else {
            return Ok(None);
        };
        if args.len() != tf.params.len()
            || args
                .iter()
                .any(|a| matches!(a, varn_tir::TirArg::Spread(_)))
        {
            return Ok(None);
        }
        if matches!(ret_expr.kind, varn_tir::TirExprKind::Closure { .. }) {
            return Ok(None);
        }

        let mut argv = Vec::with_capacity(args.len());
        for (a, &pty) in args.iter().zip(&tf.params) {
            let e = match a {
                varn_tir::TirArg::Expr(e) => e,
                varn_tir::TirArg::Named { value, .. } => value,
                varn_tir::TirArg::Spread(_) => return Ok(None),
            };
            let v = self.lower_expr(e)?;
            argv.push(self.widen_exact(v, e.ty, pty));
        }

        self.inlining_stack.push(f);
        self.inlining_params.push(argv);

        let res = self.lower_expr(ret_expr);

        self.inlining_params.pop();
        self.inlining_stack.pop();

        let v = res?;
        let coerced = self.coerce(v, call_ty);
        Ok(Some(coerced))
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
