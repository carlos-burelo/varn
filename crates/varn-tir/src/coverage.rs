//! How static the module actually is.
//!
//! The verifier says whether the module is broken. This says whether the work
//! is advancing — and with the corpus red, those are two different questions
//! that need two different instruments.

use crate::node::{
    TirArrayEl, TirExpr, TirExprKind, TirFunction, TirModule, TirObjectEntry, TirStmt,
};
use crate::resolution::Resolution;
use crate::ty::{BackendTy, DynReason};

#[derive(Debug, Default, Clone)]
pub struct Coverage {
    pub nodes: u32,
    /// Indexed in the declaration order of `DynReason`.
    dynamics: [u32; 5],
    by_name: [u32; 5],
    pub static_dispatch: u32,
    pub name_dispatch: u32,
}

fn reason_index(r: DynReason) -> usize {
    match r {
        DynReason::HostBoundary => 0,
        DynReason::Union => 1,
        DynReason::IndexSignature => 2,
        DynReason::Unannotated => 3,
        DynReason::NotYetSupported => 4,
    }
}

const REASONS: [(DynReason, &str); 5] = [
    (DynReason::HostBoundary, "host boundary"),
    (DynReason::Union, "union"),
    (DynReason::IndexSignature, "index signature"),
    (DynReason::Unannotated, "unannotated"),
    (DynReason::NotYetSupported, "not yet supported"),
];

impl Coverage {
    pub fn of(m: &TirModule) -> Coverage {
        let mut c = Coverage::default();
        c.walk_function(&m.top_level);
        for f in &m.functions {
            c.walk_function(f);
        }
        c
    }

    pub fn dynamic_by_reason(&self, r: DynReason) -> u32 {
        self.dynamics[reason_index(r)]
    }

    pub fn by_name_by_reason(&self, r: DynReason) -> u32 {
        self.by_name[reason_index(r)]
    }

    /// Share of resolutions that dispatch statically. This is the number a
    /// regression gate compares between commits.
    pub fn static_ratio(&self) -> f64 {
        let total = self.static_dispatch + self.name_dispatch;
        if total == 0 {
            return 1.0;
        }
        self.static_dispatch as f64 / total as f64
    }

    pub fn report(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!(
            "nodes {}   static {}   by name {}   static {:.0}%\n",
            self.nodes,
            self.static_dispatch,
            self.name_dispatch,
            self.static_ratio() * 100.0
        ));
        for (r, label) in REASONS {
            let d = self.dynamic_by_reason(r);
            let n = self.by_name_by_reason(r);
            if d > 0 || n > 0 {
                s.push_str(&format!("  {label:<18} dynamic {d:>5}   by name {n:>5}\n"));
            }
        }
        s
    }

    fn walk_function(&mut self, f: &TirFunction) {
        for s in &f.body {
            self.walk_stmt(s);
        }
    }

    fn walk_stmt(&mut self, s: &TirStmt) {
        match s {
            TirStmt::Expr(e) => self.walk_expr(e),
            TirStmt::Let { init, .. } => {
                if let Some(e) = init {
                    self.walk_expr(e);
                }
            }
            TirStmt::Return(v) => {
                if let Some(e) = v {
                    self.walk_expr(e);
                }
            }
            TirStmt::If { cond, then_body, else_body } => {
                self.walk_expr(cond);
                for s in then_body.iter().chain(else_body) {
                    self.walk_stmt(s);
                }
            }
            TirStmt::Loop { cond, body } => {
                self.walk_expr(cond);
                for s in body {
                    self.walk_stmt(s);
                }
            }
            TirStmt::Try { body, catch_body, .. } => {
                for s in body.iter().chain(catch_body) {
                    self.walk_stmt(s);
                }
            }
            TirStmt::Break => {}
            TirStmt::Continue => {}
            TirStmt::Throw(e) => self.walk_expr(e),
        }
    }

    fn walk_expr(&mut self, e: &TirExpr) {
        self.nodes += 1;

        if let BackendTy::Dynamic(r) = e.ty {
            self.dynamics[reason_index(r)] += 1;
        }

        match &e.res {
            Resolution::None => {}
            Resolution::ByName { why, .. } => {
                self.name_dispatch += 1;
                self.by_name[reason_index(*why)] += 1;
            }
            _ => self.static_dispatch += 1,
        }

        match &e.kind {
            TirExprKind::Binary { lhs, rhs, .. } => {
                self.walk_expr(lhs);
                self.walk_expr(rhs);
            }
            TirExprKind::Unary { operand, .. } | TirExprKind::Cast { operand } => {
                self.walk_expr(operand)
            }
            TirExprKind::Field { object, .. } => self.walk_expr(object),
            TirExprKind::Index { object, index } => {
                self.walk_expr(object);
                self.walk_expr(index);
            }
            TirExprKind::Call { callee, args } => {
                self.walk_expr(callee);
                for a in args {
                    self.walk_expr(a.value());
                }
            }
            TirExprKind::MethodCall { recv, args, .. } => {
                self.walk_expr(recv);
                for a in args {
                    self.walk_expr(a.value());
                }
            }
            TirExprKind::Assign { target, value } => {
                self.walk_expr(target);
                self.walk_expr(value);
            }
            TirExprKind::TupleLit(xs) => {
                for x in xs {
                    self.walk_expr(x);
                }
            }
            TirExprKind::ArrayLit(els) => {
                for el in els {
                    match el {
                        TirArrayEl::Expr(x) | TirArrayEl::Spread(x) => self.walk_expr(x),
                        TirArrayEl::Hole => {}
                    }
                }
            }
            TirExprKind::ObjectLit { entries } => {
                for entry in entries {
                    match entry {
                        TirObjectEntry::Field { value, .. } | TirObjectEntry::Spread(value) => {
                            self.walk_expr(value)
                        }
                    }
                }
            }
            TirExprKind::New { args, .. } | TirExprKind::MakeVariant { args } => {
                for a in args {
                    self.walk_expr(a.value());
                }
            }
            TirExprKind::Await { future } => self.walk_expr(future),
            TirExprKind::Yield { value, .. } => {
                if let Some(v) = value {
                    self.walk_expr(v);
                }
            }
            TirExprKind::Discriminant { value }
            | TirExprKind::VariantPayload { value, .. }
            | TirExprKind::TypeTest { value, .. } => self.walk_expr(value),
            TirExprKind::Select { cond, then_val, else_val } => {
                self.walk_expr(cond);
                self.walk_expr(then_val);
                self.walk_expr(else_val);
            }
            TirExprKind::ObjectKeys { operand } => self.walk_expr(operand),
            TirExprKind::SuperCall { args } | TirExprKind::SuperMethodCall { args, .. } => {
                for a in args { self.walk_expr(a.value()); }
            }
            TirExprKind::RangeLit { start, end, .. } => {
                self.walk_expr(start);
                self.walk_expr(end);
            }
            TirExprKind::DecimalLit(_) | TirExprKind::BigIntLit(_) => {}
            TirExprKind::ObjectRest { object, .. } => self.walk_expr(object),
            TirExprKind::ExtensionCall { recv, args, .. } => {
                self.walk_expr(recv);
                for a in args { self.walk_expr(a.value()); }
            }
            TirExprKind::IntLit(_)
            | TirExprKind::FloatLit(_)
            | TirExprKind::BoolLit(_)
            | TirExprKind::StrLit(_)
            | TirExprKind::CharLit(_)
            | TirExprKind::NullLit
            | TirExprKind::Closure { .. }
            | TirExprKind::Var => {}
        }
    }
}
