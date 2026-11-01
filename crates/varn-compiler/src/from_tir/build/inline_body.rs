use super::context::{Builder, InlineBody, InlineFrame, Result};
use super::functions::absent_param;
use crate::hir::LocalId;
use crate::ssa::ir::{InstKind, Terminator, Value, VarId};
use crate::OptError;
use varn_tir::{BackendTy, FnId, Resolution, TirArg, TirExpr, TirExprKind, TirFunction, TirStmt};

const NODE_BUDGET: usize = 96;
const LOCAL_SPAN: u32 = 1024;
const RESULT_SLOT: u32 = LOCAL_SPAN - 1;

struct Census {
    nodes: usize,
    inlinable: bool,
}

impl Census {
    fn stmts(&mut self, stmts: &[TirStmt]) {
        for stmt in stmts {
            self.stmt(stmt);
        }
    }

    fn stmt(&mut self, stmt: &TirStmt) {
        self.nodes += 1;
        match stmt {
            TirStmt::Expr(e) | TirStmt::Throw(e) => self.expr(e),
            TirStmt::Let { init, .. } => init.iter().for_each(|e| self.expr(e)),
            TirStmt::Return(value) => value.iter().for_each(|e| self.expr(e)),
            TirStmt::If {
                cond,
                then_body,
                else_body,
            } => {
                self.inlinable &= absent_param(cond).is_none();
                self.expr(cond);
                self.stmts(then_body);
                self.stmts(else_body);
            }
            TirStmt::Loop { cond, body } => {
                self.expr(cond);
                self.stmts(body);
            }
            TirStmt::Break | TirStmt::Continue => {}
            TirStmt::Try { .. } | TirStmt::BuildClass(_) => self.inlinable = false,
        }
    }

    fn expr(&mut self, e: &TirExpr) {
        self.nodes += 1;
        match &e.kind {
            TirExprKind::Closure { .. }
            | TirExprKind::Await { .. }
            | TirExprKind::Yield { .. }
            | TirExprKind::SuperCall { .. }
            | TirExprKind::SuperMethodCall { .. } => self.inlinable = false,
            TirExprKind::Var => {
                self.inlinable &= !matches!(e.res, Resolution::Upvalue(_));
            }
            TirExprKind::Seq { stmts, .. } => self.stmts(stmts),
            TirExprKind::IntLit(_)
            | TirExprKind::FloatLit(_)
            | TirExprKind::BoolLit(_)
            | TirExprKind::StrLit(_)
            | TirExprKind::CharLit(_)
            | TirExprKind::NullLit
            | TirExprKind::Binary { .. }
            | TirExprKind::Unary { .. }
            | TirExprKind::Field { .. }
            | TirExprKind::Index { .. }
            | TirExprKind::Call { .. }
            | TirExprKind::MethodCall { .. }
            | TirExprKind::Assign { .. }
            | TirExprKind::ArrayLit(_)
            | TirExprKind::TupleLit(_)
            | TirExprKind::ObjectLit { .. }
            | TirExprKind::RecordLit { .. }
            | TirExprKind::Discriminant { .. }
            | TirExprKind::VariantPayload { .. }
            | TirExprKind::TypeTest { .. }
            | TirExprKind::Cast { .. }
            | TirExprKind::Checked { .. }
            | TirExprKind::New { .. }
            | TirExprKind::MakeVariant { .. }
            | TirExprKind::Select { .. }
            | TirExprKind::RangeLit { .. }
            | TirExprKind::ObjectRest { .. }
            | TirExprKind::ExtensionCall { .. }
            | TirExprKind::ObjectKeys { .. }
            | TirExprKind::Length { .. }
            | TirExprKind::DecimalLit(_)
            | TirExprKind::BigIntLit(_) => {}
        }
        for child in crate::from_tir::tir_children::child_exprs(e) {
            self.expr(child);
        }
    }
}

fn body_inlinable(tf: &TirFunction, force: bool) -> bool {
    if tf.is_async
        || tf.is_generator
        || tf.has_rest
        || tf.locals.len() + tf.params.len() >= RESULT_SLOT as usize
    {
        return false;
    }
    let mut census = Census {
        nodes: 0,
        inlinable: true,
    };
    census.stmts(&tf.body);
    census.inlinable && (force || census.nodes <= NODE_BUDGET)
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

    pub(super) fn local_declared(&self, id: u32) -> Option<BackendTy> {
        match self.inline_body() {
            Some(body) => self
                .tir
                .function(body.func)
                .and_then(|tf| tf.locals.get(id as usize).copied()),
            None => self.locals_bt.get(id as usize).copied(),
        }
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
        for (a, &pty) in args.iter().zip(&tf.params) {
            let e = match a {
                TirArg::Expr(e) | TirArg::Named { value: e, .. } => e,
                TirArg::Spread(_) => return Ok(None),
            };
            argv.push(self.lower_inlined_arg(e, pty)?);
        }

        self.inline_serial += 1;
        let base = self.inline_serial * LOCAL_SPAN;
        let param_base = base + tf.locals.len() as u32;
        for (i, v) in argv.iter().enumerate() {
            self.store_var(VarId::Local(LocalId(param_base + i as u32)), *v);
        }
        let exit = self.new_block();
        self.inlining.push(InlineFrame {
            func: f,
            params: argv,
            this,
            body: Some(InlineBody {
                func: f,
                base,
                param_base,
                exit,
                result: VarId::Local(LocalId(base + RESULT_SLOT)),
                ret_ty: tf.return_ty,
            }),
        });
        let saved_return = self.return_bt.replace(tf.return_ty);
        let lowered = self.lower_block(&tf.body);
        if lowered.is_ok() && self.is_open() {
            self.leave_inlined(None);
        }
        self.return_bt = saved_return;
        let frame = self.inlining.pop();
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
            | BackendTy::Dict(_)
            | BackendTy::Set(_)
            | BackendTy::Tuple(_)
            | BackendTy::Class(_)
            | BackendTy::Nullable(_) => self.emit(InstKind::ConstNull, ty),
        }
    }
}
