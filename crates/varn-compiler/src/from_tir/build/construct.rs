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
        if let Some(func) = self.inlinable_constructor(def, args.len()) {
            let argv = self.lower_ctor_args(func, args)?;
            let inst = self.emit(
                InstKind::AllocInstance {
                    class: cv,
                    payload_size,
                },
                ty,
            );
            self.inline_constructor(func, argv, inst)?;
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

    fn inlinable_constructor(&self, def: &TirClassDef, argc: usize) -> Option<FnId> {
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
        (plain && tf.params.len() == argc && tf.body.iter().all(is_field_store))
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
            let v = self.lower_expr(e)?;
            argv.push(self.widen_exact(v, e.ty, pty));
        }
        Ok(argv)
    }

    fn inline_constructor(&mut self, func: FnId, params: Vec<Value>, this: Value) -> Result<()> {
        let ctor = self
            .tir
            .function(func)
            .ok_or(OptError::Unsupported("from_tir: constructor out of range"))?;
        self.inlining.push(InlineFrame {
            func,
            params,
            this: Some(this),
        });
        let lowered = ctor.body.iter().try_for_each(|s| self.lower_stmt(s));
        self.inlining.pop();
        lowered
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
