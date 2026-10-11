use super::context::{Builder, InlineFrame, Result};
use crate::hir::HirType;
use crate::ssa::ir::{InstKind, Value};
use crate::OptError;
use std::sync::Arc;
use varn_tir::{ClassId, FnId, Resolution, TirArg, TirClassDef, TirExpr, TirExprKind, TirStmt};

const CONSTRUCTOR: &str = "constructor";

impl<'m> Builder<'m> {
    pub(super) fn lower_new_expr(
        &mut self,
        class: ClassId,
        args: &[TirArg],
        ty: HirType,
    ) -> Result<Value> {
        let (name, payload_size) = self
            .tir
            .class(class)
            .map(|ci| (ci.name.clone(), ci.layout.payload_size))
            .ok_or(OptError::Unsupported("from_tir: New class out of range"))?;
        let cv = self.emit(self.global_load(&name), HirType::Ref);
        let static_def = self
            .class_def(class)
            .filter(|def| def.decorators.is_empty() && self.ancestry_is_local(def))
            .filter(|_| !args.iter().any(|a| matches!(a, TirArg::Spread(_))));
        let Some(def) = static_def else {
            return self.lower_call(cv, args, ty);
        };
        if let Some(func) = self.inlinable_constructor(def, args) {
            let argv = self.lower_ctor_args(func, args)?;
            let inst = self.emit(
                InstKind::AllocInstance {
                    class: cv,
                    payload_size,
                },
                ty,
            );
            self.inline_constructor(func, args, argv, inst)?;
            return Ok(inst);
        }
        let argv = self.lower_args(args)?;
        let inst = self.emit(
            InstKind::AllocInstance {
                class: cv,
                payload_size,
            },
            ty,
        );
        if self.chain_has_constructor(def) {
            self.emit(
                InstKind::MethodCall {
                    recv: inst,
                    name: Arc::from(CONSTRUCTOR),
                    args: argv,
                },
                HirType::Dynamic,
            );
        }
        Ok(inst)
    }

    fn class_def(&self, class: ClassId) -> Option<&'m TirClassDef> {
        self.tir
            .class_defs
            .iter()
            .find(|d| d.class_id == Some(class))
    }

    fn ancestry_is_local(&self, def: &TirClassDef) -> bool {
        let mut cur = def;
        for _ in 0..self.tir.class_defs.len() {
            match (cur.parent, &cur.super_class) {
                (None, None) => return true,
                (Some(p), _) => match self.class_def(p) {
                    Some(parent) if parent.decorators.is_empty() => cur = parent,
                    _ => return false,
                },
                (None, Some(_)) => return false,
            }
        }
        false
    }

    fn chain_has_constructor(&self, def: &TirClassDef) -> bool {
        let mut cur = Some(def);
        while let Some(d) = cur {
            if d.methods
                .iter()
                .any(|m| !m.is_static && m.key.as_ref() == CONSTRUCTOR)
            {
                return true;
            }
            cur = d.parent.and_then(|p| self.class_def(p));
        }
        false
    }

    fn inlinable_constructor(&self, def: &TirClassDef, args: &[TirArg]) -> Option<FnId> {
        if def.parent.is_some() || self.inlining.len() >= 4 {
            return None;
        }
        let member = def
            .methods
            .iter()
            .find(|m| !m.is_static && m.key.as_ref() == CONSTRUCTOR)?;
        if self.inlining.iter().any(|f| f.func == member.func) {
            return None;
        }
        let tf = self.tir.function(member.func)?;
        let plain = !tf.is_async && !tf.is_generator && !tf.has_rest && tf.locals.is_empty();
        let settled = |stmt: &TirStmt| match default_fill(stmt) {
            Some((param, _)) => args
                .get(param as usize)
                .is_some_and(|arg| supplied(arg).is_some()),
            None => is_field_store(stmt),
        };
        (plain && tf.params.len() == args.len() && tf.body.iter().all(settled))
            .then_some(member.func)
    }

    fn lower_ctor_args(&mut self, func: FnId, args: &[TirArg]) -> Result<Vec<Value>> {
        let ctor = self
            .tir
            .function(func)
            .ok_or(OptError::Unsupported("from_tir: constructor out of range"))?;
        let mut argv = Vec::with_capacity(args.len());
        for (a, &pty) in args.iter().zip(&ctor.params) {
            let e = match a {
                TirArg::Expr(e) | TirArg::Named { value: e, .. } => e,
                TirArg::Spread(_) => return Err(OptError::Unsupported("from_tir: spread in new")),
            };
            argv.push(self.lower_inlined_arg(e, pty)?);
        }
        Ok(argv)
    }

    fn inline_constructor(
        &mut self,
        func: FnId,
        args: &[TirArg],
        params: Vec<Value>,
        this: Value,
    ) -> Result<()> {
        let ctor = self
            .tir
            .function(func)
            .ok_or(OptError::Unsupported("from_tir: constructor out of range"))?;
        self.inlining.push(InlineFrame {
            func,
            params,
            this: Some(this),
        });
        let lowered = ctor
            .body
            .iter()
            .try_for_each(|s| self.lower_inlined_ctor_stmt(s, ctor, args));
        self.inlining.pop();
        lowered
    }

    fn lower_inlined_ctor_stmt(
        &mut self,
        stmt: &TirStmt,
        ctor: &varn_tir::TirFunction,
        args: &[TirArg],
    ) -> Result<()> {
        let Some((param, default)) = default_fill(stmt) else {
            return self.lower_stmt(stmt);
        };
        let at = param as usize;
        if args.get(at).and_then(supplied) != Some(false) {
            return Ok(());
        }
        let declared = ctor
            .params
            .get(at)
            .map(|ty| ty.non_nullable(&self.tir.types))
            .ok_or(OptError::Unsupported("from_tir: default out of range"))?;
        let filled = self.lower_inlined_arg(default, declared)?;
        if let Some(slot) = self
            .inlining
            .last_mut()
            .and_then(|frame| frame.params.get_mut(at))
        {
            *slot = filled;
        }
        Ok(())
    }
}

fn default_fill(stmt: &TirStmt) -> Option<(u32, &TirExpr)> {
    let TirStmt::If {
        cond,
        then_body,
        else_body,
    } = stmt
    else {
        return None;
    };
    let param = super::functions::absent_param(cond)?;
    let [TirStmt::Expr(fill)] = then_body.as_slice() else {
        return None;
    };
    let TirExprKind::Assign { target, value } = &fill.kind else {
        return None;
    };
    let same = matches!(target.kind, TirExprKind::Var) && target.res == Resolution::Param(param);
    (same && else_body.is_empty() && !needs_own_frame(value)).then_some((param, value.as_ref()))
}

fn supplied(arg: &TirArg) -> Option<bool> {
    let TirArg::Expr(given) = arg else {
        return None;
    };
    let shaped = matches!(
        given.ty,
        varn_tir::BackendTy::Nullable(_) | varn_tir::BackendTy::Dynamic(_)
    );
    let given = if let (TirExprKind::Cast { operand }, true) = (&given.kind, shaped) {
        operand.as_ref()
    } else {
        given
    };
    if matches!(given.kind, TirExprKind::NullLit) {
        return Some(false);
    }
    match given.ty {
        varn_tir::BackendTy::Nullable(_)
        | varn_tir::BackendTy::Dynamic(_)
        | varn_tir::BackendTy::Void
        | varn_tir::BackendTy::Never => None,
        varn_tir::BackendTy::Int
        | varn_tir::BackendTy::Float
        | varn_tir::BackendTy::Bool
        | varn_tir::BackendTy::Char
        | varn_tir::BackendTy::Str
        | varn_tir::BackendTy::Bytes
        | varn_tir::BackendTy::Decimal
        | varn_tir::BackendTy::BigInt
        | varn_tir::BackendTy::Array(_)
        | varn_tir::BackendTy::Map(..)
        | varn_tir::BackendTy::Dict(_)
        | varn_tir::BackendTy::Set(_)
        | varn_tir::BackendTy::Tuple(_)
        | varn_tir::BackendTy::Class(_)
        | varn_tir::BackendTy::Enum(_)
        | varn_tir::BackendTy::Fn(_) => Some(true),
    }
}

fn is_field_store(stmt: &TirStmt) -> bool {
    let TirStmt::Expr(e) = stmt else {
        return false;
    };
    let TirExprKind::Assign { target, value } = &e.kind else {
        return false;
    };
    let TirExprKind::Field { object, .. } = &target.kind else {
        return false;
    };
    is_this(object) && !needs_own_frame(value)
}

fn is_this(e: &TirExpr) -> bool {
    matches!(e.kind, TirExprKind::Var) && matches!(e.res, Resolution::None)
}

fn needs_own_frame(e: &TirExpr) -> bool {
    matches!(
        e.kind,
        TirExprKind::Closure { .. } | TirExprKind::Seq { .. }
    ) || super::super::tir_children::child_exprs(e)
        .into_iter()
        .any(needs_own_frame)
}
