use super::context::{Builder, InlineFrame, Result};
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
        let Some(f) = self.self_fn else {
            return Ok(None);
        };
        let declared = self.tir.function(f).map_or(0, |tf| tf.params.len());
        let argv = match self.lower_args_as_declared(f, args)? {
            Some(argv) => argv,
            None if args.len() == declared
                && !args
                    .iter()
                    .any(|a| matches!(a, varn_tir::TirArg::Spread(_))) =>
            {
                self.lower_args(args)?
            }
            None => return Ok(None),
        };
        Ok(Some(self.emit(InstKind::SelfCall { args: argv }, ty)))
    }

    pub(super) fn try_inline_direct_call(
        &mut self,
        f: varn_tir::FnId,
        args: &[varn_tir::TirArg],
        call_ty: HirType,
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
        if !tf.locals.is_empty() {
            if force {
                return Err(crate::OptError::Unsupported(
                    "`@inline` cannot apply to functions that declare locals",
                ));
            }
            return Ok(None);
        }
        if tf.body.len() != 1 {
            if force {
                return Err(crate::OptError::Unsupported(
                    "`@inline` requires a single-`return` body",
                ));
            }
            return Ok(None);
        }
        let TirStmt::Return(Some(ref ret_expr)) = tf.body[0] else {
            if force {
                return Err(crate::OptError::Unsupported(
                    "`@inline` requires a single-`return` body",
                ));
            }
            return Ok(None);
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
            _ => arg,
        };
        let v = self.lower_expr(precise)?;
        Ok(self.widen_exact(v, precise.ty, param))
    }

    pub(super) fn lower_args_as_declared(
        &mut self,
        f: varn_tir::FnId,
        args: &[varn_tir::TirArg],
    ) -> Result<Option<Vec<Value>>> {
        let Some(declared) = self
            .tir
            .function(f)
            .filter(|tf| !tf.has_rest && args.len() <= tf.params.len())
            .map(|tf| tf.params.clone())
        else {
            return Ok(None);
        };
        if args
            .iter()
            .any(|a| matches!(a, varn_tir::TirArg::Spread(_)))
        {
            return Ok(None);
        }
        let mut argv = self.lower_args(args)?;
        for (i, param) in declared.into_iter().enumerate() {
            let Some(supplied) = argv.get(i).copied() else {
                argv.push(self.emit(InstKind::ConstNull, HirType::Dynamic));
                continue;
            };
            match self.ty(param) {
                boxed @ (HirType::Dynamic | HirType::Nullable(_)) => {
                    argv[i] = self.coerce(supplied, boxed)
                }
                HirType::Int
                | HirType::Float
                | HirType::Bool
                | HirType::Str
                | HirType::Ref
                | HirType::Array(_)
                | HirType::Map(..)
                | HirType::Set(_)
                | HirType::Tuple
                | HirType::Class(_) => {}
            }
        }
        Ok(Some(argv))
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
