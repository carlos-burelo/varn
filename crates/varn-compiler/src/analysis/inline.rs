//! Trivial-function inlining registry.
//!
//! Detects top-level functions cheap enough to inline at their call sites: a
//! single `return <expr>;` body, simple by-value identifier params, no
//! generics/async/generator, and not self-recursive. The codegen consults this
//! in `compile_call` and, instead of emitting a real `Call`, evaluates the
//! arguments into registers, binds the params to those registers, and compiles
//! the body expression inline.
//!
//! No optimizer IR is required: the inlined body is compiled by the normal
//! expression codegen. Because every argument is evaluated exactly once (into a
//! register) before the body is compiled, a param appearing any number of times
//! in the body cannot duplicate or reorder argument side effects — so no
//! use-count restriction is needed. Termination across (mutually) recursive
//! inlines is guaranteed by a depth limit at the call site.

use rustc_hash::FxHashMap;
use std::rc::Rc;
use varn_core::ast::{Arg, Decl, Expr, ExprKind, FunctionDecl, Pattern, Program, StmtKind};

/// Functions with more params than this are never inlined (keeps register
/// pressure and code growth bounded).
const MAX_PARAMS: usize = 8;

pub struct InlineFn {
    /// Parameter names, in order, to bind to the argument registers.
    pub params: Vec<Rc<str>>,
    /// The single returned expression (cloned from the function body).
    pub body: Expr,
}

#[derive(Default)]
pub struct InlineRegistry {
    fns: FxHashMap<Rc<str>, InlineFn>,
}

impl InlineRegistry {
    pub fn analyze(program: &Program) -> Self {
        let mut reg = InlineRegistry::default();
        for stmt in &program.body {
            if let StmtKind::Decl(decl) = &stmt.kind {
                if let Decl::Function(f) = decl.as_ref() {
                    if let Some(inl) = Self::try_make(f) {
                        reg.fns.insert(f.id.clone(), inl);
                    }
                }
            }
        }
        reg
    }

    fn try_make(f: &FunctionDecl) -> Option<InlineFn> {
        if f.modifiers.is_async || f.modifiers.is_generator || f.modifiers.is_native {
            return None;
        }
        if !f.type_params.is_empty() || f.params.len() > MAX_PARAMS {
            return None;
        }

        // Only simple, required, by-value identifier params.
        let mut params = Vec::with_capacity(f.params.len());
        for p in &f.params {
            if p.is_rest || p.is_optional || p.default.is_some() {
                return None;
            }
            match &p.pattern {
                Pattern::Identifier { name, .. } => params.push(name.clone()),
                _ => return None,
            }
        }

        // Body must be exactly `{ return <expr>; }`.
        let stmts = match &f.body.kind {
            StmtKind::Block { stmts } => stmts,
            _ => return None,
        };
        if stmts.len() != 1 {
            return None;
        }
        let body = match &stmts[0].kind {
            StmtKind::Return {
                argument: Some(e), ..
            } => e.as_ref().clone(),
            _ => return None,
        };

        // Reject self-recursion (would inline without bound up to the depth
        // limit, just bloating code). The call-site depth limit is the hard
        // termination guarantee; this is the cheap early-out.
        if expr_references(&body, &f.id) {
            return None;
        }

        Some(InlineFn { params, body })
    }

    pub fn get(&self, name: &str) -> Option<&InlineFn> {
        self.fns.get(name)
    }

    pub fn is_empty(&self) -> bool {
        self.fns.is_empty()
    }
}

/// Best-effort check: does `expr` reference the identifier `name` anywhere?
/// Used only to skip obviously-recursive functions; a missed case is still
/// bounded by the call-site inline-depth limit.
fn expr_references(expr: &Expr, name: &str) -> bool {
    match &expr.kind {
        ExprKind::Identifier { name: n } => n.as_ref() == name,
        ExprKind::Binary { left, right, .. } | ExprKind::Logical { left, right, .. } => {
            expr_references(left, name) || expr_references(right, name)
        }
        ExprKind::Unary { operand, .. } => expr_references(operand, name),
        ExprKind::Paren { expression, .. } => expr_references(expression, name),
        ExprKind::Assign { target, value, .. } => {
            expr_references(target, name) || expr_references(value, name)
        }
        ExprKind::Conditional {
            test,
            consequent,
            alternate,
        } => {
            expr_references(test, name)
                || expr_references(consequent, name)
                || expr_references(alternate, name)
        }
        ExprKind::Member {
            object, property, ..
        } => expr_references(object, name) || expr_references(property, name),
        ExprKind::Call { callee, args, .. } => {
            expr_references(callee, name)
                || args.iter().any(|a| {
                    let e = match a {
                        Arg::Positional(e) | Arg::Spread(e) => e,
                        Arg::Named { value, .. } => value,
                    };
                    expr_references(e, name)
                })
        }
        _ => false,
    }
}
