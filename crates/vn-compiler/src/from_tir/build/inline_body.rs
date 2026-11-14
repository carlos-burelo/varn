use super::context::{Builder, InlineBody, InlineFrame, Result};
use crate::hir::LocalId;
use crate::ssa::ir::{InstKind, Terminator, Value, VarId};
use crate::OptError;
use vn_tir::{BackendTy, FnId, TirArg, TirFunction};

const LOCAL_SPAN: u32 = 1024;
const RESULT_SLOT: u32 = LOCAL_SPAN - 1;

fn body_inlinable(tf: &TirFunction, force: bool) -> bool {
    tf.locals.len() + tf.params.len() < RESULT_SLOT as usize
        && vn_tir::inlinable::body_inlinable(tf, force)
}

impl<'m> Builder<'m> {
    fn inline_body(&self) -> Option<&InlineBody> {
        self.inlining.last().and_then(|frame| frame.body.as_ref())
    }

    pub(super) fn local_var(&self, id: u32) -> VarId {
        let base = self.inline_body().map_or(0, |body| body.base);
        VarId::Local(LocalId(base + id))
    }

    pub(super) fn inlined_param_var(&self, index: u32) -> Option<VarId> {
        let body = self.inline_body()?;
        Some(VarId::Local(LocalId(body.param_base + index)))
    }

    pub(super) fn try_inline_body_call(
        &mut self,
        f: FnId,
        args: &[TirArg],
        call_ty: BackendTy,
        force: bool,
        this: Option<Value>,
    ) -> Result<Option<Value>> {
        let tir = self.tir;
        let Some(tf) = tir.function(f) else {
            return Ok(None);
        };
        let plain_args = args.len() == tf.params.len()
            && tf.has_this == this.is_some()
            && !args.iter().any(|a| matches!(a, TirArg::Spread(_)));
        if !plain_args || !body_inlinable(tf, force) {
            if force {
                return Err(OptError::Unsupported(
                    "`@inline` needs a body without closures, try, await, yield or default parameters",
                ));
            }
            return Ok(None);
        }

        let mut argv = Vec::with_capacity(args.len());
        for a in args {
            let e = match a {
                TirArg::Expr(e) | TirArg::Named { value: e, .. } => e,
                TirArg::Spread(_) => return Ok(None),
            };
            argv.push(self.lower_inlined_arg(e)?);
        }

        self.inline_serial += 1;
        let base = self.inline_serial * LOCAL_SPAN;
        let param_base = base + tf.locals.len() as u32;
        for (i, v) in argv.iter().enumerate() {
            self.store_var(VarId::Local(LocalId(param_base + i as u32)), *v);
        }
        let exit = self.new_block();
        self.push_inline(InlineFrame {
            func: f,
            params: argv,
            this,
            body: Some(InlineBody {
                base,
                param_base,
                exit,
                result: VarId::Local(LocalId(base + RESULT_SLOT)),
                ret_ty: tf.return_ty,
            }),
        });
        let lowered = self.lower_block(&tf.body);
        if lowered.is_ok() && self.is_open() {
            self.leave_inlined(None);
        }
        let frame = self.pop_inline();
        lowered?;

        let Some(body) = frame.and_then(|f| f.body) else {
            return Ok(None);
        };
        self.seal_block(body.exit);
        self.current = body.exit;
        if self.blocks[body.exit.0 as usize].preds.is_empty() {
            let dead = self.emit(InstKind::ConstNull, call_ty);
            self.set_term(Terminator::Unreachable);
            return Ok(Some(dead));
        }
        let result = self.read_var(body.result, body.exit)?;
        Ok(Some(self.coerce(result, call_ty)))
    }

    pub(super) fn leave_inlined(&mut self, value: Option<Value>) {
        let Some(body) = self.inline_body() else {
            return;
        };
        let (exit, result, ret_ty) = (body.exit, body.result, body.ret_ty);
        let value = match value {
            Some(v) => v,
            None => self.zero_value(ret_ty),
        };
        let value = self.coerce(value, ret_ty);
        self.store_var(result, value);
        let from = self.current;
        self.set_term(Terminator::Jump {
            target: exit,
            args: vec![],
        });
        self.add_pred(exit, from);
    }

    pub(super) fn zero_value(&mut self, ty: BackendTy) -> Value {
        match ty {
            BackendTy::Literal(_) => self.zero_value(ty.representation()),
            BackendTy::Int => self.emit(InstKind::ConstInt(0), ty),
            BackendTy::Float => self.emit(InstKind::ConstFloat(0.0), ty),
            BackendTy::Bool => self.emit(InstKind::ConstBool(false), ty),
            BackendTy::Str
            | BackendTy::Char
            | BackendTy::Bytes
            | BackendTy::Enum(_)
            | BackendTy::Fn(_)
            | BackendTy::Dynamic(_)
            | BackendTy::Decimal
            | BackendTy::BigInt
            | BackendTy::Void
            | BackendTy::Never
            | BackendTy::Array(_)
            | BackendTy::Map(..)
            | BackendTy::Set(_)
            | BackendTy::Tuple(_)
            | BackendTy::Class(_)
            | BackendTy::Nullable(_) => self.emit(InstKind::ConstNull, ty),
        }
    }
}
