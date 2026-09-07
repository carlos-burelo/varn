//! Function bodies.
//!
//! Sub-phase 2a: literals, `Identifier`, `let`, `return`, `if`, `while`,
//! `throw`, `break` / `continue`, expression statements, and the scalar-safe
//! subset of `Binary` / `Unary`. Everything else — calls, member access,
//! `match`, C-style `for`, `for…of` — lowers to a `Dynamic(NotYetSupported)`
//! placeholder, never a half-built node the verifier cannot check.

use crate::checker::TypeEntry;
use crate::emit::tables::NameIndex;
use crate::emit::ty::{lower_type, NameResolver};
use rustc_hash::{FxHashMap, FxHashSet};
use std::rc::Rc;
use varn_core::ast::operators::{BinaryOp, LogicalOp, UnaryOp};
use varn_core::ast::pattern::{MatchBinding, MatchPattern};
use varn_core::ast::{
    Arg, ArrayEl, AstId, Expr, ExprKind, MatchBody, MatchCase, ObjectProp, Pattern, PropKey, Stmt,
    StmtKind,
};
use varn_tir::{
    BackendTy, ClassId, ClassInfo, DynReason, EnumInfo, LocalId, Resolution, Signature, SigId, Span,
    TirArg, TirArrayEl, TirBinOp, TirExpr, TirExprKind, TirFunction, TirObjectEntry, TirStmt,
    TirUnOp, TyTable,
};

/// The module-wide handles a body emitter needs but does not own.
#[derive(Clone, Copy)]
pub(super) struct ModuleCtx<'a> {
    pub names: &'a NameIndex,
    pub classes: &'a [ClassInfo],
    pub enums: &'a [EnumInfo],
    /// Module value symbol name → global slot.
    pub globals: &'a FxHashMap<Rc<str>, u32>,
    /// Free-function name → (index into `TirModule::functions`, arity).
    pub fns: &'a FxHashMap<Rc<str>, (u32, u32)>,
}

pub(super) struct FnEmitter<'a> {
    pub expr_table: &'a FxHashMap<AstId, TypeEntry>,
    pub tt: &'a mut TyTable,
    m: ModuleCtx<'a>,
    pub signatures: &'a mut Vec<Signature>,
    /// Closure bodies produced while lowering this function. Their `FnId` is
    /// `closure_base + <index in this vector at push time>`; the caller
    /// appends this whole vector to `TirModule::functions` at `closure_base`.
    out_closures: &'a mut Vec<TirFunction>,
    closure_base: u32,
    pub locals: Vec<BackendTy>,
    scopes: Vec<FxHashMap<Rc<str>, LocalId>>,
    params: Vec<Rc<str>>,
    this_class: Option<ClassId>,
    /// Names visible in an enclosing function (this emitter is a closure body).
    /// A reference to one of them resolves to an `Upvalue` rather than
    /// `ByName`.
    outer_names: FxHashSet<Rc<str>>,
    /// Distinct captured names, in first-reference order; the `Upvalue` index.
    captures: Vec<Rc<str>>,
    /// Statements produced while lowering an expression (hoisted temps, match
    /// desugaring). `lower_stmt` drains this in front of the statement it was
    /// lowering.
    pending: Vec<TirStmt>,
}

enum ClosureBody<'a> {
    Expr(&'a Expr),
    Stmt(&'a Stmt),
}

fn pattern_lead(p: &Pattern) -> Rc<str> {
    match p {
        Pattern::Identifier { name, .. } => name.clone(),
        _ => Rc::from("_"),
    }
}

/// What a `match` arm does with its value.
#[derive(Clone, Copy)]
enum MatchDest {
    Statement,
    Return,
    Assign(LocalId),
}

fn span_of(e: &Expr) -> Span {
    Span { start: e.range.start.offset, end: e.range.end.offset }
}

fn placeholder(reason: DynReason) -> TirExpr {
    TirExpr {
        kind: TirExprKind::NullLit,
        ty: BackendTy::Dynamic(reason),
        res: Resolution::None,
        span: Span::EMPTY,
    }
}

impl<'a> FnEmitter<'a> {
    pub fn new(
        expr_table: &'a FxHashMap<AstId, TypeEntry>,
        tt: &'a mut TyTable,
        m: ModuleCtx<'a>,
        signatures: &'a mut Vec<Signature>,
        out_closures: &'a mut Vec<TirFunction>,
        closure_base: u32,
        params: Vec<Rc<str>>,
    ) -> Self {
        FnEmitter {
            expr_table,
            tt,
            m,
            signatures,
            out_closures,
            closure_base,
            locals: Vec::new(),
            scopes: vec![FxHashMap::default()],
            params,
            this_class: None,
            outer_names: FxHashSet::default(),
            captures: Vec::new(),
            pending: Vec::new(),
        }
    }

    /// A synthetic local with no source name, for a hoisted temp.
    fn fresh_local(&mut self, ty: BackendTy) -> LocalId {
        let id = LocalId(self.locals.len() as u32);
        self.locals.push(ty);
        id
    }

    /// Bind `e` to a fresh local via a pending `Let`, and return a `Var` that
    /// reads it. Used where an expression must be mentioned more than once but
    /// is not pure.
    fn hoist(&mut self, e: TirExpr) -> TirExpr {
        let ty = e.ty;
        let span = e.span;
        let local = self.fresh_local(ty);
        self.pending.push(TirStmt::Let { local, ty, init: Some(e) });
        TirExpr { kind: TirExprKind::Var, ty, res: Resolution::Local(local), span }
    }

    pub fn with_this(mut self, class: ClassId) -> Self {
        self.this_class = Some(class);
        self
    }

    fn class_of(&self, ty: BackendTy) -> Option<&'a ClassInfo> {
        match ty.non_nullable(self.tt) {
            BackendTy::Class(c) => self.m.classes.get(c.0 as usize),
            _ => None,
        }
    }

    /// The type the checker CHECKED this expression against, lowered. The
    /// `refined` lane is deliberately not used: it can be strictly stronger
    /// than the declared type (an evolved empty array proved `int[]` inside a
    /// `: str` function), and feeding it to a node the verifier then checks
    /// for coherence turns an optimisation into a spurious error. `refined`
    /// re-enters as an opt-only channel in a later sub-phase.
    fn expr_ty(&mut self, e: &Expr) -> BackendTy {
        let names = self.m.names;
        match self.expr_table.get(&e.id) {
            Some(entry) => lower_type(&entry.ty, self.tt, names),
            None => BackendTy::Dynamic(DynReason::Unannotated),
        }
    }

    fn resolve_name(&mut self, name: &str) -> Resolution {
        for scope in self.scopes.iter().rev() {
            if let Some(id) = scope.get(name) {
                return Resolution::Local(*id);
            }
        }
        if let Some(i) = self.params.iter().position(|p| p.as_ref() == name) {
            return Resolution::Param(i as u32);
        }
        // A name from an enclosing function: this is a closure capture.
        if self.outer_names.contains(name) {
            let idx = match self.captures.iter().position(|c| c.as_ref() == name) {
                Some(i) => i,
                None => {
                    self.captures.push(Rc::from(name));
                    self.captures.len() - 1
                }
            };
            return Resolution::Upvalue(idx as u32);
        }
        if let Some(&slot) = self.m.globals.get(name) {
            return Resolution::GlobalSlot(slot);
        }
        Resolution::ByName { name: Rc::from(name), why: DynReason::Unannotated }
    }

    fn bind_local(&mut self, name: Rc<str>, ty: BackendTy) -> LocalId {
        let id = LocalId(self.locals.len() as u32);
        self.locals.push(ty);
        self.scopes.last_mut().unwrap().insert(name, id);
        id
    }

    pub fn lower_block(&mut self, stmts: &[Stmt]) -> Vec<TirStmt> {
        self.scopes.push(FxHashMap::default());
        let mut out = Vec::new();
        for s in stmts {
            out.extend(self.lower_stmt(s));
        }
        self.scopes.pop();
        out
    }

    pub fn lower_stmt_as_block(&mut self, s: &Stmt) -> Vec<TirStmt> {
        match &s.kind {
            StmtKind::Block { stmts } => self.lower_block(stmts),
            _ => self.lower_stmt(s),
        }
    }

    /// Lower one source statement into zero or more TIR statements. Hoisted
    /// temps from the statement's own expressions are prepended; control-flow
    /// arms that recurse into sub-statements drain `pending` themselves before
    /// recursing.
    fn lower_stmt(&mut self, s: &Stmt) -> Vec<TirStmt> {
        let one = |s: TirStmt| vec![s];
        let drained = |em: &mut Self, built: Vec<TirStmt>| {
            let mut out = std::mem::take(&mut em.pending);
            out.extend(built);
            out
        };
        match &s.kind {
            StmtKind::Block { stmts } => self.lower_block(stmts),
            StmtKind::Expr { expression } => {
                if let ExprKind::Match { subject, cases } = &expression.kind {
                    return self.lower_match_stmt(subject, cases);
                }
                let e = self.lower_expr(expression);
                drained(self, one(TirStmt::Expr(e)))
            }
            StmtKind::Empty | StmtKind::Debugger | StmtKind::Error => vec![],

            StmtKind::Decl(decl) => {
                let built = self.lower_decl_stmt(decl);
                drained(self, built)
            }

            StmtKind::Return { argument } => {
                if let Some(arg) = argument {
                    if let ExprKind::Match { subject, cases } = &arg.kind {
                        return self.lower_match(subject, cases, MatchDest::Return);
                    }
                }
                let a = argument.as_ref().map(|a| self.lower_expr(a));
                drained(self, one(TirStmt::Return(a)))
            }
            StmtKind::Throw { argument } => {
                let a = self.lower_expr(argument);
                drained(self, one(TirStmt::Throw(a)))
            }
            StmtKind::Break { .. } => one(TirStmt::Break),
            StmtKind::Continue { .. } => one(TirStmt::Continue),

            StmtKind::If { test, consequent, alternate } => {
                let cond = self.lower_cond(test);
                let mut out = std::mem::take(&mut self.pending);
                let then_body = self.lower_stmt_as_block(consequent);
                let else_body =
                    alternate.as_ref().map(|a| self.lower_stmt_as_block(a)).unwrap_or_default();
                out.push(TirStmt::If { cond, then_body, else_body });
                out
            }

            StmtKind::While { test, body } => {
                let cond = self.lower_cond(test);
                let cond_pending = std::mem::take(&mut self.pending);
                let body = self.lower_stmt_as_block(body);
                if cond_pending.is_empty() {
                    one(TirStmt::Loop { cond, body })
                } else {
                    // The condition needs a temp; recompute it each iteration
                    // as a gate at the top of the loop body.
                    let mut loop_body = cond_pending;
                    loop_body.push(TirStmt::If {
                        cond,
                        then_body: vec![],
                        else_body: vec![TirStmt::Break],
                    });
                    loop_body.extend(body);
                    one(TirStmt::Loop { cond: bool_lit(true), body: loop_body })
                }
            }

            StmtKind::For { init, test, update, body } => self.lower_for(
                init.as_deref(),
                test.as_deref(),
                update.as_deref(),
                body,
            ),

            StmtKind::DoWhile { body, test } => {
                let mut loop_body = self.lower_stmt_as_block(body);
                let cond = self.lower_cond(test);
                loop_body.extend(std::mem::take(&mut self.pending));
                loop_body.push(TirStmt::If {
                    cond,
                    then_body: vec![],
                    else_body: vec![TirStmt::Break],
                });
                one(TirStmt::Loop { cond: bool_lit(true), body: loop_body })
            }

            StmtKind::ForOf { left, right, body, .. } => self.lower_for_of(left, right, body),

            StmtKind::Try { block, catches, finally } => {
                self.lower_try(block, catches, finally.as_deref())
            }

            StmtKind::Switch { discriminant, cases } => {
                self.lower_switch(discriminant, cases)
            }

            // for-in, using, labeled: later.
            _ => one(TirStmt::Expr(placeholder(DynReason::NotYetSupported))),
        }
    }

    /// `try { } catch (e) { } finally { }`. `finally` has no TIR node — its
    /// statements are appended after the `Try` (correct for straight-line and
    /// caught paths, not for a `return`/`throw` that escapes the try).
    fn lower_try(
        &mut self,
        block: &Stmt,
        catches: &[varn_core::ast::CatchClause],
        finally: Option<&Stmt>,
    ) -> Vec<TirStmt> {
        let body = self.lower_stmt_as_block(block);
        let (catch_local, catch_body) = match catches.first() {
            Some(c) => {
                let name = match &c.param {
                    Some(Pattern::Identifier { name, .. }) => name.clone(),
                    _ => Rc::from("<catch>"),
                };
                let local = self.bind_local(name, BackendTy::Dynamic(DynReason::NotYetSupported));
                let cb = self.lower_stmt_as_block(&c.body);
                (local, cb)
            }
            None => {
                let local = self.fresh_local(BackendTy::Dynamic(DynReason::NotYetSupported));
                (local, vec![])
            }
        };
        let mut out = vec![TirStmt::Try { body, catch_local, catch_body }];
        if let Some(f) = finally {
            out.extend(self.lower_stmt_as_block(f));
        }
        out
    }

    /// `switch (d) { case a: … case b: … default: … }` -> hoist `d`, then an
    /// If-chain of `d == case`. Fallthrough is not modelled — each case is
    /// assumed to `break`.
    fn lower_switch(
        &mut self,
        discriminant: &Expr,
        cases: &[varn_core::ast::SwitchCase],
    ) -> Vec<TirStmt> {
        let d = self.lower_expr(discriminant);
        let mut out = std::mem::take(&mut self.pending);
        let d = self.hoist(d);
        out.extend(std::mem::take(&mut self.pending));
        out.extend(self.switch_cases(&d, cases, 0));
        out
    }

    fn switch_cases(
        &mut self,
        d: &TirExpr,
        cases: &[varn_core::ast::SwitchCase],
        i: usize,
    ) -> Vec<TirStmt> {
        let Some(case) = cases.get(i) else { return vec![] };
        self.scopes.push(FxHashMap::default());
        let body: Vec<TirStmt> = case.body.iter().flat_map(|s| self.lower_stmt(s)).collect();
        self.scopes.pop();
        let rest = self.switch_cases(d, cases, i + 1);
        match &case.test {
            None => {
                // default: runs unconditionally at this position.
                let mut v = body;
                v.extend(rest);
                v
            }
            Some(t) => {
                let te = self.lower_expr(t);
                let mut pre = std::mem::take(&mut self.pending);
                let cond = TirExpr {
                    kind: TirExprKind::Binary {
                        op: TirBinOp::Eq,
                        lhs: Box::new(d.clone()),
                        rhs: Box::new(te),
                    },
                    ty: BackendTy::Bool,
                    res: Resolution::None,
                    span: d.span,
                };
                pre.push(TirStmt::If { cond, then_body: body, else_body: rest });
                pre
            }
        }
    }

    /// `for (init; test; update) body` -> the init, then a `Loop` whose body
    /// runs `update` at the top (gated by a first-iteration flag, so `continue`
    /// still advances), then the test as a `break` gate, then the body.
    fn lower_for(
        &mut self,
        init: Option<&varn_core::ast::ForInit>,
        test: Option<&Expr>,
        update: Option<&Expr>,
        body: &Stmt,
    ) -> Vec<TirStmt> {
        use varn_core::ast::ForInit;
        let mut out = Vec::new();

        match init {
            Some(ForInit::Var { declarators, .. }) => {
                for d in declarators {
                    if let Pattern::Identifier { name, .. } = &d.id {
                        let iexpr = d.init.as_ref().map(|e| self.lower_expr(e));
                        let ty = iexpr
                            .as_ref()
                            .map(|e| e.ty)
                            .unwrap_or(BackendTy::Dynamic(DynReason::Unannotated));
                        let local = self.bind_local(name.clone(), ty);
                        out.extend(std::mem::take(&mut self.pending));
                        out.push(TirStmt::Let { local, ty, init: iexpr });
                    }
                }
            }
            Some(ForInit::Expr(e)) => {
                let e = self.lower_expr(e);
                out.extend(std::mem::take(&mut self.pending));
                out.push(TirStmt::Expr(e));
            }
            None => {}
        }

        let first = self.fresh_local(BackendTy::Bool);
        out.push(TirStmt::Let {
            local: first,
            ty: BackendTy::Bool,
            init: Some(bool_lit(true)),
        });
        let first_var = || TirExpr {
            kind: TirExprKind::Var,
            ty: BackendTy::Bool,
            res: Resolution::Local(first),
            span: Span::EMPTY,
        };

        let mut loop_body: Vec<TirStmt> = Vec::new();

        // update, skipped on the first iteration
        if let Some(u) = update {
            let ue = self.lower_expr(u);
            let mut upd = std::mem::take(&mut self.pending);
            upd.push(TirStmt::Expr(ue));
            loop_body.push(TirStmt::If {
                cond: TirExpr {
                    kind: TirExprKind::Unary {
                        op: TirUnOp::Not,
                        operand: Box::new(first_var()),
                    },
                    ty: BackendTy::Bool,
                    res: Resolution::None,
                    span: Span::EMPTY,
                },
                then_body: upd,
                else_body: vec![],
            });
        }
        loop_body.push(TirStmt::Expr(TirExpr {
            kind: TirExprKind::Assign {
                target: Box::new(first_var()),
                value: Box::new(bool_lit(false)),
            },
            ty: BackendTy::Void,
            res: Resolution::None,
            span: Span::EMPTY,
        }));

        // test -> break gate
        if let Some(t) = test {
            let cond = self.lower_cond(t);
            loop_body.extend(std::mem::take(&mut self.pending));
            loop_body.push(TirStmt::If {
                cond,
                then_body: vec![],
                else_body: vec![TirStmt::Break],
            });
        }

        loop_body.extend(self.lower_stmt_as_block(body));
        out.push(TirStmt::Loop { cond: bool_lit(true), body: loop_body });
        out
    }

    /// `for (x of iterable) body`. Only an `Array` iterable is lowered here —
    /// `let i = 0; loop { if !(i < len) break; let x = arr[i]; body; i = i+1 }`;
    /// the iterator protocol for other types is a later sub-phase.
    fn lower_for_of(&mut self, left: &Pattern, right: &Expr, body: &Stmt) -> Vec<TirStmt> {
        let Pattern::Identifier { name, .. } = left else {
            return vec![TirStmt::Expr(placeholder(DynReason::NotYetSupported))];
        };
        let iter = self.lower_expr(right);
        let mut out = std::mem::take(&mut self.pending);

        let BackendTy::Array(el) = iter.ty.non_nullable(self.tt) else {
            return vec![TirStmt::Expr(placeholder(DynReason::NotYetSupported))];
        };
        let elem_ty = self.tt.get(el);
        let arr = self.hoist(iter);
        out.extend(std::mem::take(&mut self.pending));

        let idx = self.fresh_local(BackendTy::Int);
        out.push(TirStmt::Let {
            local: idx,
            ty: BackendTy::Int,
            init: Some(int_lit(0)),
        });
        let idx_var = || TirExpr {
            kind: TirExprKind::Var,
            ty: BackendTy::Int,
            res: Resolution::Local(idx),
            span: Span::EMPTY,
        };

        let len = self.field_access(arr.clone(), Rc::from("length"), BackendTy::Int, Span::EMPTY);
        let cond = TirExpr {
            kind: TirExprKind::Binary {
                op: TirBinOp::Lt,
                lhs: Box::new(idx_var()),
                rhs: Box::new(len),
            },
            ty: BackendTy::Bool,
            res: Resolution::None,
            span: Span::EMPTY,
        };

        let elem = TirExpr {
            kind: TirExprKind::Index {
                object: Box::new(arr),
                index: Box::new(idx_var()),
            },
            ty: elem_ty,
            res: Resolution::None,
            span: Span::EMPTY,
        };
        let x_local = self.bind_local(name.clone(), elem_ty);

        let mut loop_body = vec![
            TirStmt::If { cond, then_body: vec![], else_body: vec![TirStmt::Break] },
            TirStmt::Let { local: x_local, ty: elem_ty, init: Some(elem) },
        ];
        loop_body.extend(self.lower_stmt_as_block(body));
        loop_body.push(TirStmt::Expr(TirExpr {
            kind: TirExprKind::Assign {
                target: Box::new(idx_var()),
                value: Box::new(TirExpr {
                    kind: TirExprKind::Binary {
                        op: TirBinOp::Add,
                        lhs: Box::new(idx_var()),
                        rhs: Box::new(int_lit(1)),
                    },
                    ty: BackendTy::Int,
                    res: Resolution::None,
                    span: Span::EMPTY,
                }),
            },
            ty: BackendTy::Void,
            res: Resolution::None,
            span: Span::EMPTY,
        }));

        out.push(TirStmt::Loop { cond: bool_lit(true), body: loop_body });
        out
    }

    /// `match subject { … }` -> hoist the subject, then a chain of `If`.
    /// `dest` says what each arm does with its value: nothing (statement
    /// position), `return` it, or assign it to a local (`let x = match …`). A
    /// pattern this sub-phase does not support, or an impure guard, degrades
    /// the whole match to one placeholder.
    fn lower_match(&mut self, subject: &Expr, cases: &[MatchCase], dest: MatchDest) -> Vec<TirStmt> {
        let subj = self.lower_expr(subject);
        let mut out = std::mem::take(&mut self.pending);

        let all_ok = cases.iter().all(|c| {
            pattern_supported(&c.pattern)
                && c.guard.as_ref().map(Self::is_pure).unwrap_or(true)
        });
        if !all_ok {
            out.push(match dest {
                MatchDest::Statement | MatchDest::Assign(_) => {
                    TirStmt::Expr(placeholder(DynReason::NotYetSupported))
                }
                MatchDest::Return => TirStmt::Return(Some(placeholder(DynReason::NotYetSupported))),
            });
            return out;
        }

        let s = self.hoist(subj);
        out.extend(std::mem::take(&mut self.pending));
        let chain = self.match_cases(&s, cases, 0, dest);
        out.extend(chain);
        out
    }

    fn lower_match_stmt(&mut self, subject: &Expr, cases: &[MatchCase]) -> Vec<TirStmt> {
        self.lower_match(subject, cases, MatchDest::Statement)
    }

    fn match_cases(
        &mut self,
        s: &TirExpr,
        cases: &[MatchCase],
        i: usize,
        dest: MatchDest,
    ) -> Vec<TirStmt> {
        let Some(case) = cases.get(i) else { return vec![] };

        self.scopes.push(FxHashMap::default());
        let (mut cond, mut then_body) = self.match_pattern(s, &case.pattern);
        if let Some(g) = &case.guard {
            let gexpr = self.lower_expr(g);
            cond = and_bool(cond, gexpr); // guard is pure, no pending
        }
        // The arm's value expression, then what `dest` does with it.
        let value = match &case.body {
            MatchBody::Expr(e) => self.lower_expr(e),
            MatchBody::Block(stmt) => {
                // A block-bodied arm in value position is not lowered yet;
                // run it for effect and yield null.
                then_body.extend(self.lower_stmt_as_block(stmt));
                TirExpr {
                    kind: TirExprKind::NullLit,
                    ty: BackendTy::Dynamic(DynReason::NotYetSupported),
                    res: Resolution::None,
                    span: s.span,
                }
            }
        };
        then_body.extend(std::mem::take(&mut self.pending));
        match dest {
            MatchDest::Statement => then_body.push(TirStmt::Expr(value)),
            MatchDest::Return => then_body.push(TirStmt::Return(Some(value))),
            MatchDest::Assign(local) => then_body.push(TirStmt::Expr(TirExpr {
                kind: TirExprKind::Assign {
                    target: Box::new(TirExpr {
                        kind: TirExprKind::Var,
                        ty: value.ty,
                        res: Resolution::Local(local),
                        span: s.span,
                    }),
                    value: Box::new(value),
                },
                ty: BackendTy::Void,
                res: Resolution::None,
                span: s.span,
            })),
        }
        self.scopes.pop();

        let else_body = self.match_cases(s, cases, i + 1, dest);
        vec![TirStmt::If { cond, then_body, else_body }]
    }

    /// Test `s` against one pattern: a `Bool` condition and the bindings the
    /// pattern introduces, as `Let` statements to run when it matches.
    fn match_pattern(&mut self, s: &TirExpr, pat: &MatchPattern) -> (TirExpr, Vec<TirStmt>) {
        match pat {
            MatchPattern::Wildcard => (bool_lit(true), vec![]),
            MatchPattern::Identifier(name) => {
                // A bare name against an enum subject is a nullary variant
                // test, not a binding.
                if let BackendTy::Enum(eid) = s.ty.non_nullable(self.tt) {
                    if let Some(info) = self.m.enums.get(eid.0 as usize) {
                        if info.variants.iter().any(|v| v.name.as_ref() == name.as_ref()) {
                            return self.match_enum_variant(s, name, name, &[]);
                        }
                    }
                }
                let local = self.bind_local(name.clone(), s.ty);
                (bool_lit(true), vec![TirStmt::Let { local, ty: s.ty, init: Some(s.clone()) }])
            }
            MatchPattern::Literal(lit) => {
                let l = self.lower_expr(lit);
                let cond = TirExpr {
                    kind: TirExprKind::Binary {
                        op: TirBinOp::Eq,
                        lhs: Box::new(s.clone()),
                        rhs: Box::new(l),
                    },
                    ty: BackendTy::Bool,
                    res: Resolution::None,
                    span: s.span,
                };
                (cond, vec![])
            }
            MatchPattern::Type { type_name, binding } => {
                let Some(cid) = self.m.names.class_id(type_name) else {
                    return (bool_lit(false), vec![]);
                };
                let cond = TirExpr {
                    kind: TirExprKind::TypeTest { value: Box::new(s.clone()), class: cid },
                    ty: BackendTy::Bool,
                    res: Resolution::None,
                    span: s.span,
                };
                let mut binds = vec![];
                if let Some(name) = binding {
                    let local = self.bind_local(name.clone(), BackendTy::Class(cid));
                    binds.push(TirStmt::Let {
                        local,
                        ty: BackendTy::Class(cid),
                        init: Some(s.clone()),
                    });
                }
                (cond, binds)
            }
            MatchPattern::EnumVariant { enum_name, variant_name, bindings } => {
                self.match_enum_variant(s, enum_name, variant_name, bindings)
            }
            // Record / Sequence: pattern_supported already rejected these.
            _ => (bool_lit(false), vec![]),
        }
    }

    fn match_enum_variant(
        &mut self,
        s: &TirExpr,
        enum_name: &str,
        variant_name: &str,
        bindings: &[MatchBinding],
    ) -> (TirExpr, Vec<TirStmt>) {
        // The pattern may name the enum (`Color.Red`) or just the variant
        // (`Red`), in which case the subject's own type says which enum.
        let eid = self.m.names.enum_id(enum_name).or_else(|| match s.ty.non_nullable(self.tt) {
            BackendTy::Enum(e) => Some(e),
            _ => None,
        });
        let Some(eid) = eid else {
            return (bool_lit(false), vec![]);
        };
        let Some(info) = self.m.enums.get(eid.0 as usize) else {
            return (bool_lit(false), vec![]);
        };
        let Some(variant) = info.variants.iter().find(|v| v.name.as_ref() == variant_name) else {
            return (bool_lit(false), vec![]);
        };
        let tag = variant.tag;
        let payload: Vec<BackendTy> = variant.payload.clone();

        let disc = TirExpr {
            kind: TirExprKind::Discriminant { value: Box::new(s.clone()) },
            ty: BackendTy::Int,
            res: Resolution::None,
            span: s.span,
        };
        let cond = TirExpr {
            kind: TirExprKind::Binary {
                op: TirBinOp::Eq,
                lhs: Box::new(disc),
                rhs: Box::new(TirExpr {
                    kind: TirExprKind::IntLit(tag as i64),
                    ty: BackendTy::Int,
                    res: Resolution::None,
                    span: s.span,
                }),
            },
            ty: BackendTy::Bool,
            res: Resolution::None,
            span: s.span,
        };

        let mut binds = Vec::new();
        for (i, b) in bindings.iter().enumerate() {
            let fty = payload.get(i).copied().unwrap_or(BackendTy::Dynamic(DynReason::NotYetSupported));
            let field = TirExpr {
                kind: TirExprKind::VariantPayload {
                    value: Box::new(s.clone()),
                    tag,
                    field: i as u16,
                },
                ty: fty,
                res: Resolution::EnumVariant { enum_id: eid, tag },
                span: s.span,
            };
            let local = self.bind_local(b.name.clone(), fty);
            binds.push(TirStmt::Let { local, ty: fty, init: Some(field) });
        }
        (cond, binds)
    }

    fn lower_decl_stmt(&mut self, decl: &varn_core::ast::Decl) -> Vec<TirStmt> {
        use varn_core::ast::{Decl, ExportDecl};
        let v = match decl {
            Decl::Variable(v) => v,
            Decl::Export(ExportDecl::Decl { declaration, .. }) => match declaration.as_ref() {
                Decl::Variable(v) => v,
                _ => return vec![], // nested fn/class/enum: handled at module level
            },
            _ => return vec![],
        };
        let mut out = Vec::new();
        for d in &v.declarators {
            match &d.id {
                Pattern::Identifier { name, .. } => {
                    // `let x = match … { … }` — declare `x`, then let the arms
                    // assign it.
                    if let Some(init) = &d.init {
                        if let ExprKind::Match { subject, cases } = &init.kind {
                            let ty = self
                                .expr_table
                                .get(&init.id)
                                .map(|e| {
                                    let names = self.m.names;
                                    lower_type(&e.ty, self.tt, names)
                                })
                                .unwrap_or(BackendTy::Dynamic(DynReason::Unannotated));
                            let local = self.bind_local(name.clone(), ty);
                            out.push(TirStmt::Let { local, ty, init: None });
                            out.extend(std::mem::take(&mut self.pending));
                            out.extend(self.lower_match(
                                subject,
                                cases,
                                MatchDest::Assign(local),
                            ));
                            continue;
                        }
                    }

                    let init = d.init.as_ref().map(|e| self.lower_expr(e));
                    let ty = init
                        .as_ref()
                        .map(|e| e.ty)
                        .unwrap_or(BackendTy::Dynamic(DynReason::Unannotated));
                    let local = self.bind_local(name.clone(), ty);
                    out.extend(std::mem::take(&mut self.pending));
                    out.push(TirStmt::Let { local, ty, init });
                }
                // Destructuring: `let {a,b} = obj` / `let [x,y] = arr`.
                pat => {
                    let Some(init) = &d.init else {
                        out.push(TirStmt::Expr(placeholder(DynReason::NotYetSupported)));
                        continue;
                    };
                    let src = self.lower_expr(init);
                    out.extend(std::mem::take(&mut self.pending));
                    let src = self.hoist(src);
                    out.extend(std::mem::take(&mut self.pending));
                    self.bind_pattern(pat, src, &mut out);
                }
            }
        }
        out
    }

    /// Statements that unpack every non-trivial parameter pattern
    /// (`fn f({a, b})`) into locals, to run before the body.
    pub fn destructure_params(&mut self, params: &[varn_core::ast::Param]) -> Vec<TirStmt> {
        let mut out = Vec::new();
        for (i, p) in params.iter().enumerate() {
            if matches!(p.pattern, Pattern::Identifier { .. }) {
                continue;
            }
            let src = TirExpr {
                kind: TirExprKind::Var,
                ty: BackendTy::Dynamic(DynReason::Unannotated),
                res: Resolution::Param(i as u32),
                span: Span::EMPTY,
            };
            self.bind_pattern(&p.pattern, src, &mut out);
        }
        out
    }

    /// Bind a (possibly nested) destructuring pattern against an
    /// already-lowered, side-effect-free source expression.
    fn bind_pattern(&mut self, pat: &Pattern, src: TirExpr, out: &mut Vec<TirStmt>) {
        match pat {
            Pattern::Identifier { name, .. } => {
                let local = self.bind_local(name.clone(), src.ty);
                out.push(TirStmt::Let { local, ty: src.ty, init: Some(src) });
            }
            Pattern::Object { properties, .. } => {
                for prop in properties {
                    let field = self.field_access(
                        src.clone(),
                        prop.key.clone(),
                        BackendTy::Dynamic(DynReason::Unannotated),
                        src.span,
                    );
                    self.bind_pattern(&prop.value, field, out);
                }
            }
            Pattern::Array { elements, .. } => {
                // The verifier pins an array index's type to the element type.
                let elem_ty = match src.ty.non_nullable(self.tt) {
                    BackendTy::Array(e) => self.tt.get(e),
                    _ => BackendTy::Dynamic(DynReason::Unannotated),
                };
                for (i, slot) in elements.iter().enumerate() {
                    let Some(el) = slot else { continue }; // hole
                    let idx = TirExpr {
                        kind: TirExprKind::Index {
                            object: Box::new(src.clone()),
                            index: Box::new(int_lit(i as i64)),
                        },
                        ty: elem_ty,
                        res: Resolution::None,
                        span: src.span,
                    };
                    self.bind_pattern(&el.pattern, idx, out);
                }
            }
            Pattern::Assignment { left, right, .. } => {
                // `{ a = default }` — a null subject falls back to the default.
                let def = self.lower_expr(right);
                out.extend(std::mem::take(&mut self.pending));
                let value = if def.ty == src.ty || matches!(src.ty, BackendTy::Dynamic(_)) {
                    let is_null = TirExpr {
                        kind: TirExprKind::Unary {
                            op: TirUnOp::IsNull,
                            operand: Box::new(src.clone()),
                        },
                        ty: BackendTy::Bool,
                        res: Resolution::None,
                        span: src.span,
                    };
                    TirExpr {
                        kind: TirExprKind::Select {
                            cond: Box::new(is_null),
                            then_val: Box::new(def),
                            else_val: Box::new(src.clone()),
                        },
                        ty: src.ty,
                        res: Resolution::None,
                        span: src.span,
                    }
                } else {
                    src.clone() // arm types disagree; skip the default
                };
                self.bind_pattern(left, value, out);
            }
            // Rest patterns need a slice primitive.
            Pattern::Rest { .. } => {
                out.push(TirStmt::Expr(placeholder(DynReason::NotYetSupported)));
            }
        }
    }

    /// A condition the verifier will require to be `Bool`. If the checker
    /// typed it as anything else we still emit it — the coverage report wants
    /// the truth — but a non-Bool, non-Dynamic condition would fail coherence,
    /// so those degrade to a placeholder.
    fn lower_cond(&mut self, e: &Expr) -> TirExpr {
        let lowered = self.lower_expr(e);
        match lowered.ty {
            BackendTy::Bool | BackendTy::Dynamic(_) => lowered,
            _ => placeholder(DynReason::NotYetSupported),
        }
    }

    fn lower_expr(&mut self, e: &Expr) -> TirExpr {
        let ty = self.expr_ty(e);
        let span = span_of(e);

        let kind = match &e.kind {
            ExprKind::IntLiteral { value, .. } => Some(TirExprKind::IntLit(*value)),
            ExprKind::FloatLiteral { value, .. } => Some(TirExprKind::FloatLit(*value)),
            ExprKind::BoolLiteral { value } => Some(TirExprKind::BoolLit(*value)),
            ExprKind::StrLiteral { value } => Some(TirExprKind::StrLit(Rc::from(value.as_str()))),
            ExprKind::CharLiteral { value } => Some(TirExprKind::CharLit(*value)),
            ExprKind::NullLiteral => Some(TirExprKind::NullLit),

            ExprKind::Identifier { name } => {
                return TirExpr { kind: TirExprKind::Var, ty, res: self.resolve_name(name), span }
            }

            ExprKind::Paren { expression } => return self.lower_expr(expression),

            ExprKind::Binary { op, left, right } => {
                return self.lower_binary(*op, left, right, ty, span)
            }
            ExprKind::Unary { op, operand, prefix: _ } => {
                return self.lower_unary(*op, operand, ty, span)
            }

            ExprKind::This => {
                let this_ty = self
                    .this_class
                    .map(BackendTy::Class)
                    .unwrap_or(BackendTy::Dynamic(DynReason::NotYetSupported));
                return TirExpr { kind: TirExprKind::Var, ty: this_ty, res: Resolution::None, span };
            }

            ExprKind::Member { object, property, computed, optional } => {
                return self.lower_member(object, property, *computed, *optional, ty, span)
            }

            ExprKind::Logical { op, left, right } => {
                return self.lower_logical(*op, left, right, ty, span)
            }

            ExprKind::Template { parts } => return self.lower_template(parts, span),

            ExprKind::Function { params, body, is_async, is_generator, .. } => {
                return self.lower_closure(params, ClosureBody::Stmt(body), *is_async, *is_generator, ty, span)
            }
            ExprKind::Arrow { params, body, is_async, .. } => {
                let cb = match body.as_ref() {
                    varn_core::ast::ArrowBody::Expr(e) => ClosureBody::Expr(e),
                    varn_core::ast::ArrowBody::Block(s) => ClosureBody::Stmt(s),
                };
                return self.lower_closure(params, cb, *is_async, false, ty, span);
            }

            ExprKind::Await { argument } => {
                let fut = self.lower_expr(argument);
                return TirExpr {
                    kind: TirExprKind::Await { future: Box::new(fut) },
                    ty,
                    res: Resolution::None,
                    span,
                };
            }
            ExprKind::Yield { argument, delegate } => {
                let value = argument.as_ref().map(|a| Box::new(self.lower_expr(a)));
                return TirExpr {
                    kind: TirExprKind::Yield { value, delegate: *delegate },
                    // The resume value is not typed yet.
                    ty: BackendTy::Dynamic(DynReason::NotYetSupported),
                    res: Resolution::None,
                    span,
                };
            }

            ExprKind::Call { callee, args, optional: _, type_args: _ } => {
                return self.lower_call(callee, args, ty, span)
            }

            ExprKind::Array { elements } => {
                let els = elements
                    .iter()
                    .map(|el| match el {
                        ArrayEl::Expr(e) => TirArrayEl::Expr(self.lower_expr(e)),
                        ArrayEl::Spread(e) => TirArrayEl::Spread(self.lower_expr(e)),
                        ArrayEl::Hole => TirArrayEl::Hole,
                    })
                    .collect();
                return TirExpr {
                    kind: TirExprKind::ArrayLit(els),
                    ty,
                    res: Resolution::None,
                    span,
                };
            }
            ExprKind::Tuple { elements } => {
                let xs = elements.iter().map(|e| self.lower_expr(e)).collect();
                return TirExpr {
                    kind: TirExprKind::TupleLit(xs),
                    ty,
                    res: Resolution::None,
                    span,
                };
            }
            ExprKind::Object { properties } | ExprKind::Record { properties } => {
                let entries = properties
                    .iter()
                    .filter_map(|p| match p {
                        ObjectProp::Property { key, value, .. } => Some(TirObjectEntry::Field {
                            name: prop_key_name(key)?,
                            value: self.lower_expr(value),
                        }),
                        ObjectProp::Spread { argument, .. } => {
                            Some(TirObjectEntry::Spread(self.lower_expr(argument)))
                        }
                        // Methods / getters / setters in an object literal are
                        // a later sub-phase.
                        _ => None,
                    })
                    .collect();
                return TirExpr {
                    kind: TirExprKind::ObjectLit { entries },
                    ty,
                    res: Resolution::None,
                    span,
                };
            }
            ExprKind::New { callee, args, .. } => return self.lower_new(callee, args, ty, span),

            // `x!` — a non-null assertion. A `Cast` carries the type change
            // without disturbing the inner node's `res` (a `FieldSlot` read
            // must keep the field's declared type).
            ExprKind::NonNull { expression } => {
                let inner = self.lower_expr(expression);
                let nn = inner.ty.non_nullable(self.tt);
                if inner.ty == nn {
                    return inner;
                }
                return TirExpr {
                    kind: TirExprKind::Cast { operand: Box::new(inner) },
                    ty: nn,
                    res: Resolution::None,
                    span,
                };
            }
            // `x as T` — an explicit representation change. `x satisfies T` is
            // an identity check, so it is just the inner expression.
            ExprKind::As { expression, .. } => {
                let inner = self.lower_expr(expression);
                return TirExpr {
                    kind: TirExprKind::Cast { operand: Box::new(inner) },
                    ty,
                    res: Resolution::None,
                    span,
                };
            }
            ExprKind::Satisfies { expression, .. } => return self.lower_expr(expression),
            // `(a, b, c)` — run the leading expressions for effect, yield the
            // last.
            ExprKind::Sequence { expressions } => {
                let Some((last, lead)) = expressions.split_last() else {
                    return TirExpr { span, ..placeholder(DynReason::NotYetSupported) };
                };
                for e in lead {
                    let te = self.lower_expr(e);
                    self.pending.push(TirStmt::Expr(te));
                }
                return self.lower_expr(last);
            }
            // `x |> f` -> `f(x)`.
            ExprKind::Pipeline { left, right } => {
                let arg = self.lower_expr(left);
                let callee = self.lower_expr(right);
                return TirExpr {
                    kind: TirExprKind::Call {
                        callee: Box::new(callee),
                        args: vec![TirArg::Expr(arg)],
                    },
                    ty,
                    res: Resolution::ByName {
                        name: Rc::from("<pipeline>"),
                        why: DynReason::Unannotated,
                    },
                    span,
                };
            }
            // `{ ...obj, k: v }` -> an ObjectLit with a leading spread.
            ExprKind::With { object, properties } => {
                let mut entries = vec![TirObjectEntry::Spread(self.lower_expr(object))];
                for p in properties {
                    if let ObjectProp::Property { key, value, .. } = p {
                        if let Some(name) = prop_key_name(key) {
                            entries.push(TirObjectEntry::Field {
                                name,
                                value: self.lower_expr(value),
                            });
                        }
                    }
                }
                return TirExpr {
                    kind: TirExprKind::ObjectLit { entries },
                    ty,
                    res: Resolution::None,
                    span,
                };
            }

            // Assignment to an identifier or a field: plain `=` directly,
            // compound `+=` … as `t = t <op> v`. Destructuring targets later.
            ExprKind::Assign { op, target, value }
                if matches!(
                    target.kind,
                    ExprKind::Identifier { .. } | ExprKind::Member { .. }
                ) && Self::is_pure(target) =>
            {
                let t = self.lower_expr(target);
                let v = self.lower_expr(value);
                let rhs = match assign_bin_op(*op) {
                    Ok(None) => v, // plain `=`
                    Ok(Some(bop)) if operands_coherent(bop, t.ty, v.ty, t.ty) => TirExpr {
                        kind: TirExprKind::Binary {
                            op: bop,
                            lhs: Box::new(t.clone()),
                            rhs: Box::new(v),
                        },
                        ty: t.ty,
                        res: Resolution::None,
                        span,
                    },
                    _ => return TirExpr { span, ..placeholder(DynReason::NotYetSupported) },
                };
                return TirExpr {
                    kind: TirExprKind::Assign { target: Box::new(t), value: Box::new(rhs) },
                    ty,
                    res: Resolution::None,
                    span,
                };
            }

            // `x++` / `--x` -> `x = x <+/-> 1` (the value it yields is not
            // distinguished; correct in statement position, which is almost
            // always where it sits).
            ExprKind::Update { op, operand, .. }
                if matches!(
                    operand.kind,
                    ExprKind::Identifier { .. } | ExprKind::Member { .. }
                ) && Self::is_pure(operand) =>
            {
                use varn_core::ast::operators::UpdateOp;
                let t = self.lower_expr(operand);
                if t.ty != BackendTy::Int {
                    return TirExpr { span, ..placeholder(DynReason::NotYetSupported) };
                }
                let bop = match op {
                    UpdateOp::Increment => TirBinOp::Add,
                    UpdateOp::Decrement => TirBinOp::Sub,
                };
                let stepped = TirExpr {
                    kind: TirExprKind::Binary {
                        op: bop,
                        lhs: Box::new(t.clone()),
                        rhs: Box::new(int_lit(1)),
                    },
                    ty: BackendTy::Int,
                    res: Resolution::None,
                    span,
                };
                return TirExpr {
                    kind: TirExprKind::Assign {
                        target: Box::new(t),
                        value: Box::new(stepped),
                    },
                    ty: BackendTy::Int,
                    res: Resolution::None,
                    span,
                };
            }

            _ => None,
        };

        match kind {
            Some(kind) => TirExpr { kind, ty, res: Resolution::None, span },
            None => TirExpr { span, ..placeholder(DynReason::NotYetSupported) },
        }
    }

    fn lower_binary(
        &mut self,
        op: BinaryOp,
        left: &Expr,
        right: &Expr,
        ty: BackendTy,
        span: Span,
    ) -> TirExpr {
        let Some(top) = bin_op(op) else {
            return TirExpr { span, ..placeholder(DynReason::NotYetSupported) };
        };
        let lhs = self.lower_expr(left);
        let rhs = self.lower_expr(right);

        // Only emit a real Binary when the operands agree on a non-dynamic
        // scalar and the result the checker gave is coherent with it. This is
        // the "never a node the verifier cannot check" rule: a mixed int/float
        // needs an explicit Cast, which sub-phase 2b adds.
        let coherent = operands_coherent(top, lhs.ty, rhs.ty, ty);
        if !coherent {
            return TirExpr { span, ..placeholder(DynReason::NotYetSupported) };
        }
        TirExpr {
            kind: TirExprKind::Binary { op: top, lhs: Box::new(lhs), rhs: Box::new(rhs) },
            ty,
            res: Resolution::None,
            span,
        }
    }

    fn member_name(property: &Expr) -> Option<Rc<str>> {
        match &property.kind {
            ExprKind::Identifier { name } => Some(name.clone()),
            ExprKind::StrLiteral { value } => Some(Rc::from(value.as_str())),
            _ => None,
        }
    }

    /// An expression that can be lowered more than once without changing
    /// behaviour: no calls, no assignments, no construction. Used to decide
    /// whether `??` / `?.` / `&&` / `||` can desugar to a `Select` (which
    /// mentions an operand twice) without a hoisted temp.
    fn is_pure(e: &Expr) -> bool {
        match &e.kind {
            ExprKind::Identifier { .. }
            | ExprKind::This
            | ExprKind::IntLiteral { .. }
            | ExprKind::FloatLiteral { .. }
            | ExprKind::BoolLiteral { .. }
            | ExprKind::StrLiteral { .. }
            | ExprKind::CharLiteral { .. }
            | ExprKind::NullLiteral => true,
            ExprKind::Paren { expression } => Self::is_pure(expression),
            ExprKind::Member { object, property, computed, .. } => {
                Self::is_pure(object) && (!computed || Self::is_pure(property))
            }
            ExprKind::Binary { left, right, .. } => {
                Self::is_pure(left) && Self::is_pure(right)
            }
            ExprKind::Unary { operand, .. } => Self::is_pure(operand),
            _ => false,
        }
    }

    fn lower_logical(
        &mut self,
        op: LogicalOp,
        left: &Expr,
        right: &Expr,
        ty: BackendTy,
        span: Span,
    ) -> TirExpr {
        // `&&` / `||`: the right operand must not run when short-circuited, so
        // it cannot be hoisted — it has to be pure to appear inside a Select
        // arm. `??` only ever needs the left twice, and that always runs, so
        // an impure left can be hoisted to a temp.
        let (cond, then_val, else_val) = match op {
            LogicalOp::And | LogicalOp::Or => {
                if !Self::is_pure(left) || !Self::is_pure(right) {
                    return TirExpr { span, ..placeholder(DynReason::NotYetSupported) };
                }
                let l = self.lower_expr(left);
                let r = self.lower_expr(right);
                if l.ty != BackendTy::Bool || ty != BackendTy::Bool {
                    return TirExpr { span, ..placeholder(DynReason::NotYetSupported) };
                }
                match op {
                    LogicalOp::And => (l, r, bool_lit(false)), // a ? b : false
                    _ => (l, bool_lit(true), r),               // a ? true : b
                }
            }
            LogicalOp::Nullish => {
                let mut l = self.lower_expr(left);
                if !Self::is_pure(left) {
                    l = self.hoist(l);
                }
                let r = self.lower_expr(right);
                let is_null = TirExpr {
                    kind: TirExprKind::Unary {
                        op: TirUnOp::IsNull,
                        operand: Box::new(l.clone()),
                    },
                    ty: BackendTy::Bool,
                    res: Resolution::None,
                    span,
                };
                (is_null, r, l) // IsNull(a) ? b : a
            }
        };
        // A Select whose arms disagree and whose result is not a union would
        // fail coherence — degrade instead.
        if then_val.ty != else_val.ty && !matches!(ty, BackendTy::Dynamic(_)) {
            return TirExpr { span, ..placeholder(DynReason::NotYetSupported) };
        }
        TirExpr {
            kind: TirExprKind::Select {
                cond: Box::new(cond),
                then_val: Box::new(then_val),
                else_val: Box::new(else_val),
            },
            ty,
            res: Resolution::None,
            span,
        }
    }

    fn lower_member(
        &mut self,
        object: &Expr,
        property: &Expr,
        computed: bool,
        optional: bool,
        ty: BackendTy,
        span: Span,
    ) -> TirExpr {
        // `a?.b` -> IsNull(a) ? null : a.b. The receiver is used twice; hoist
        // it to a temp when it is not pure.
        if optional && !computed {
            let Some(name) = Self::member_name(property) else {
                return TirExpr { span, ..placeholder(DynReason::NotYetSupported) };
            };
            let mut recv = self.lower_expr(object);
            if !Self::is_pure(object) {
                recv = self.hoist(recv);
            }
            let is_null = TirExpr {
                kind: TirExprKind::Unary {
                    op: TirUnOp::IsNull,
                    operand: Box::new(recv.clone()),
                },
                ty: BackendTy::Bool,
                res: Resolution::None,
                span,
            };
            let access = self.field_access(recv, name, ty, span);
            if access.ty == ty || matches!(ty, BackendTy::Dynamic(_)) {
                let null_arm =
                    TirExpr { kind: TirExprKind::NullLit, ty, res: Resolution::None, span };
                return TirExpr {
                    kind: TirExprKind::Select {
                        cond: Box::new(is_null),
                        then_val: Box::new(null_arm),
                        else_val: Box::new(access),
                    },
                    ty,
                    res: Resolution::None,
                    span,
                };
            }
            return TirExpr { span, ..placeholder(DynReason::NotYetSupported) };
        }

        let obj = self.lower_expr(object);

        if computed {
            // `obj[key]`. Verifier only constrains the Array case, so only
            // emit Index when the element type lines up; otherwise placeholder.
            let index = self.lower_expr(property);
            if let BackendTy::Array(el) = obj.ty.non_nullable(self.tt) {
                if self.tt.get(el) != ty {
                    return TirExpr { span, ..placeholder(DynReason::NotYetSupported) };
                }
            }
            return TirExpr {
                kind: TirExprKind::Index { object: Box::new(obj), index: Box::new(index) },
                ty,
                res: Resolution::None,
                span,
            };
        }

        let Some(name) = Self::member_name(property) else {
            return TirExpr { span, ..placeholder(DynReason::NotYetSupported) };
        };

        // `E.V` — a unit enum variant.
        if let Some((enum_id, tag)) = self.enum_variant(object, &name) {
            return TirExpr {
                kind: TirExprKind::MakeVariant { args: vec![] },
                ty: BackendTy::Enum(enum_id),
                res: Resolution::EnumVariant { enum_id, tag },
                span,
            };
        }

        self.field_access(obj, name, ty, span)
    }

    /// Build a `Field` node from an already-lowered receiver. A known field on
    /// a class receiver takes its declared type (the authority the verifier
    /// checks `FieldSlot` against); anything else is a by-name read.
    fn field_access(&mut self, obj: TirExpr, name: Rc<str>, ty: BackendTy, span: Span) -> TirExpr {
        match self.class_of(obj.ty).and_then(|ci| ci.field(&name).cloned()) {
            Some(field) => TirExpr {
                kind: TirExprKind::Field { object: Box::new(obj), name },
                ty: field.ty,
                res: Resolution::FieldSlot(field.slot),
                span,
            },
            None => TirExpr {
                kind: TirExprKind::Field { object: Box::new(obj), name: name.clone() },
                ty,
                res: Resolution::ByName { name, why: DynReason::Unannotated },
                span,
            },
        }
    }

    /// Lower a function or arrow expression to a `Closure` node plus a
    /// `TirFunction` for its body, appended to `out_closures`. Captures are not
    /// resolved here — an outer-scope name in the body lands on `ByName`, which
    /// is the honest state until upvalue analysis is a sub-phase.
    fn lower_closure(
        &mut self,
        params: &[varn_core::ast::Param],
        body: ClosureBody,
        is_async: bool,
        is_generator: bool,
        ty: BackendTy,
        span: Span,
    ) -> TirExpr {
        let func_id = varn_tir::FnId(self.closure_base + self.out_closures.len() as u32);
        // Reserve the slot so a nested closure gets the next id.
        self.out_closures.push(TirFunction {
            name: Rc::from("<closure>"),
            sig: SigId(0),
            params: vec![],
            return_ty: BackendTy::Dynamic(DynReason::NotYetSupported),
            locals: vec![],
            body: vec![],
            has_this: false,
            this_class: None,
            is_async,
            is_generator,
        });
        let slot = self.out_closures.len() - 1;

        let param_names: Vec<Rc<str>> =
            params.iter().map(|p| pattern_lead(&p.pattern)).collect();
        let param_tys = vec![BackendTy::Dynamic(DynReason::Unannotated); params.len()];

        // Every name visible here is visible to the closure body as a capture:
        // this emitter's outer names, its scopes, and its params.
        let mut outer_names = self.outer_names.clone();
        for scope in &self.scopes {
            outer_names.extend(scope.keys().cloned());
        }
        outer_names.extend(self.params.iter().cloned());

        // The sub-emitter shares `out_closures`, and a closure's FnId is
        // `closure_base + <its index in that shared vector>`, so the base is
        // the same for nested closures.
        let mut sub = FnEmitter::new(
            self.expr_table,
            &mut *self.tt,
            self.m,
            &mut *self.signatures,
            &mut *self.out_closures,
            self.closure_base,
            param_names,
        );
        sub.outer_names = outer_names;
        let mut stmts = sub.destructure_params(params);
        stmts.extend(match body {
            ClosureBody::Stmt(s) => sub.lower_stmt_as_block(s),
            ClosureBody::Expr(e) => {
                let te = sub.lower_expr(e);
                let mut b = std::mem::take(&mut sub.pending);
                b.push(TirStmt::Return(Some(te)));
                b
            }
        });
        let locals = sub.locals;

        self.out_closures[slot] = TirFunction {
            name: Rc::from("<closure>"),
            sig: SigId(0),
            params: param_tys,
            return_ty: BackendTy::Dynamic(DynReason::NotYetSupported),
            locals,
            body: stmts,
            has_this: false,
            this_class: None,
            is_async,
            is_generator,
        };

        TirExpr { kind: TirExprKind::Closure { func: func_id }, ty, res: Resolution::None, span }
    }

    /// A template string folds to `Str` concatenation. Each interpolation
    /// that is not already `Str` gets a `Cast` to it — the verifier trusts a
    /// `Cast`, and the backend does the real conversion.
    fn lower_template(&mut self, parts: &[varn_core::ast::TemplatePart], span: Span) -> TirExpr {
        use varn_core::ast::TemplatePart;
        let str_expr = |kind, span| TirExpr { kind, ty: BackendTy::Str, res: Resolution::None, span };
        let mut acc: Option<TirExpr> = None;
        for part in parts {
            let piece = match part {
                TemplatePart::Literal(s) => {
                    str_expr(TirExprKind::StrLit(Rc::from(s.as_str())), span)
                }
                TemplatePart::Interpolation(e) => {
                    let le = self.lower_expr(e);
                    if le.ty == BackendTy::Str {
                        le
                    } else {
                        str_expr(TirExprKind::Cast { operand: Box::new(le) }, span)
                    }
                }
            };
            acc = Some(match acc {
                None => piece,
                Some(a) => str_expr(
                    TirExprKind::Binary {
                        op: TirBinOp::Add,
                        lhs: Box::new(a),
                        rhs: Box::new(piece),
                    },
                    span,
                ),
            });
        }
        acc.unwrap_or_else(|| str_expr(TirExprKind::StrLit(Rc::from("")), span))
    }

    fn lower_new(&mut self, callee: &Expr, args: &[Arg], ty: BackendTy, span: Span) -> TirExpr {
        let class = match &callee.kind {
            ExprKind::Identifier { name } => self.m.names.class_id(name),
            _ => None,
        };
        let Some(class) = class else {
            return TirExpr { span, ..placeholder(DynReason::NotYetSupported) };
        };
        let targs = args.iter().map(|a| self.lower_arg(a)).collect();
        TirExpr {
            kind: TirExprKind::New { class, args: targs },
            ty,
            res: Resolution::None,
            span,
        }
    }

    /// `E.V` or `E.V(args)` for a module enum — the tag lives in `res`.
    fn enum_variant(&self, object: &Expr, variant: &str) -> Option<(varn_tir::EnumId, u16)> {
        let ExprKind::Identifier { name } = &object.kind else { return None };
        let eid = self.m.names.enum_id(name)?;
        let info = self.m.enums.get(eid.0 as usize)?;
        let v = info.variants.iter().find(|v| v.name.as_ref() == variant)?;
        Some((eid, v.tag))
    }

    fn lower_arg(&mut self, a: &Arg) -> TirArg {
        match a {
            Arg::Positional(e) => TirArg::Expr(self.lower_expr(e)),
            Arg::Spread(e) => TirArg::Spread(self.lower_expr(e)),
            Arg::Named { label, value } => {
                TirArg::Named { label: Rc::from(label.as_str()), value: self.lower_expr(value) }
            }
        }
    }

    fn lower_call(&mut self, callee: &Expr, args: &[Arg], ty: BackendTy, span: Span) -> TirExpr {
        // Free call on an identifier: `f(args)`.
        if let ExprKind::Identifier { name } = &callee.kind {
            let c = self.lower_expr(callee);
            let targs: Vec<TirArg> = args.iter().map(|a| self.lower_arg(a)).collect();
            let all_positional = targs.iter().all(|a| matches!(a, TirArg::Expr(_)));
            let res = match self.m.fns.get(name) {
                Some(&(fn_id, arity)) if all_positional && arity as usize == targs.len() => {
                    Resolution::DirectFn(varn_tir::FnId(fn_id))
                }
                _ => Resolution::ByName { name: name.clone(), why: DynReason::Unannotated },
            };
            return TirExpr {
                kind: TirExprKind::Call { callee: Box::new(c), args: targs },
                ty,
                res,
                span,
            };
        }

        // Method call: `recv.name(args)`.
        let ExprKind::Member { object, property, computed: false, .. } = &callee.kind else {
            return TirExpr { span, ..placeholder(DynReason::NotYetSupported) };
        };
        let Some(name) = Self::member_name(property) else {
            return TirExpr { span, ..placeholder(DynReason::NotYetSupported) };
        };

        // `E.V(args)` — an enum variant with a payload.
        if let Some((enum_id, tag)) = self.enum_variant(object, &name) {
            let vargs = args.iter().map(|a| self.lower_arg(a)).collect();
            return TirExpr {
                kind: TirExprKind::MakeVariant { args: vargs },
                ty: BackendTy::Enum(enum_id),
                res: Resolution::EnumVariant { enum_id, tag },
                span,
            };
        }

        let recv = self.lower_expr(object);
        let targs: Vec<TirArg> = args.iter().map(|a| self.lower_arg(a)).collect();

        // A vtable slot only when the receiver is a class with that method and
        // the arity matches its signature — the verifier checks both. Getters
        // and setters carry decorated names in the vtable, so a plain call
        // never hits them here. Everything else is by-name.
        let all_positional = targs.iter().all(|a| matches!(a, TirArg::Expr(_)));
        let res = self
            .class_of(recv.ty)
            .and_then(|ci| ci.method_slot(&name).map(|s| (s, ci)))
            .and_then(|(slot, ci)| {
                let entry = ci.method_at(slot)?;
                let sig = self.signatures.get(entry.sig.0 as usize)?;
                (all_positional && sig.params.len() == targs.len())
                    .then_some(Resolution::VtableSlot(slot))
            })
            .unwrap_or(Resolution::ByName { name: name.clone(), why: DynReason::Unannotated });

        TirExpr {
            kind: TirExprKind::MethodCall { recv: Box::new(recv), name, args: targs },
            ty,
            res,
            span,
        }
    }

    fn lower_unary(&mut self, op: UnaryOp, operand: &Expr, ty: BackendTy, span: Span) -> TirExpr {
        let top = match op {
            UnaryOp::Minus => TirUnOp::Neg,
            UnaryOp::Not => TirUnOp::Not,
            UnaryOp::BitNot => TirUnOp::BitNot,
            UnaryOp::Plus => return self.lower_expr(operand), // unary + is identity
            UnaryOp::Typeof => return TirExpr { span, ..placeholder(DynReason::NotYetSupported) },
        };
        let inner = self.lower_expr(operand);
        let ok = match top {
            TirUnOp::Neg => matches!(inner.ty, BackendTy::Int | BackendTy::Float) && inner.ty == ty,
            TirUnOp::Not => inner.ty == BackendTy::Bool && ty == BackendTy::Bool,
            TirUnOp::BitNot => inner.ty == BackendTy::Int && ty == BackendTy::Int,
            TirUnOp::IsNull => false,
        };
        if !ok {
            return TirExpr { span, ..placeholder(DynReason::NotYetSupported) };
        }
        TirExpr {
            kind: TirExprKind::Unary { op: top, operand: Box::new(inner) },
            ty,
            res: Resolution::None,
            span,
        }
    }
}

fn pattern_supported(p: &MatchPattern) -> bool {
    matches!(
        p,
        MatchPattern::Wildcard
            | MatchPattern::Identifier(_)
            | MatchPattern::Literal(_)
            | MatchPattern::Type { .. }
            | MatchPattern::EnumVariant { .. }
    )
}

/// `a && b` as a Bool expression, via Select (`a ? b : false`).
fn and_bool(a: TirExpr, b: TirExpr) -> TirExpr {
    let span = a.span;
    if a.ty != BackendTy::Bool || b.ty != BackendTy::Bool {
        // A non-Bool guard: fall back to just the pattern condition.
        return a;
    }
    TirExpr {
        kind: TirExprKind::Select {
            cond: Box::new(a),
            then_val: Box::new(b),
            else_val: Box::new(bool_lit(false)),
        },
        ty: BackendTy::Bool,
        res: Resolution::None,
        span,
    }
}

fn prop_key_name(key: &PropKey) -> Option<Rc<str>> {
    match key {
        PropKey::Identifier(s) | PropKey::Str(s) => Some(Rc::from(s.as_str())),
        PropKey::Int(n) => Some(Rc::from(n.to_string())),
        PropKey::Computed(_) => None,
    }
}

fn bool_lit(v: bool) -> TirExpr {
    TirExpr {
        kind: TirExprKind::BoolLit(v),
        ty: BackendTy::Bool,
        res: Resolution::None,
        span: Span::EMPTY,
    }
}

/// `Ok(None)` for a plain `=`, `Ok(Some(op))` for an arithmetic compound
/// assignment, `Err` for one this sub-phase does not lower (`??=`, `&&=`,
/// bitwise-assign).
fn assign_bin_op(op: varn_core::ast::operators::AssignOp) -> Result<Option<TirBinOp>, ()> {
    use varn_core::ast::operators::AssignOp as A;
    Ok(Some(match op {
        A::Assign => return Ok(None),
        A::AddAssign => TirBinOp::Add,
        A::SubAssign => TirBinOp::Sub,
        A::MulAssign => TirBinOp::Mul,
        A::DivAssign => TirBinOp::Div,
        A::ModAssign => TirBinOp::Mod,
        A::PowAssign => TirBinOp::Pow,
        _ => return Err(()),
    }))
}

fn int_lit(v: i64) -> TirExpr {
    TirExpr {
        kind: TirExprKind::IntLit(v),
        ty: BackendTy::Int,
        res: Resolution::None,
        span: Span::EMPTY,
    }
}

fn bin_op(op: BinaryOp) -> Option<TirBinOp> {
    Some(match op {
        BinaryOp::Add => TirBinOp::Add,
        BinaryOp::Sub => TirBinOp::Sub,
        BinaryOp::Mul => TirBinOp::Mul,
        BinaryOp::Div => TirBinOp::Div,
        BinaryOp::Mod => TirBinOp::Mod,
        BinaryOp::Pow => TirBinOp::Pow,
        BinaryOp::Eq => TirBinOp::Eq,
        BinaryOp::NotEq => TirBinOp::Ne,
        BinaryOp::Lt => TirBinOp::Lt,
        BinaryOp::Gt => TirBinOp::Gt,
        BinaryOp::LtEq => TirBinOp::Le,
        BinaryOp::GtEq => TirBinOp::Ge,
        BinaryOp::BitAnd => TirBinOp::BitAnd,
        BinaryOp::BitOr => TirBinOp::BitOr,
        BinaryOp::BitXor => TirBinOp::BitXor,
        BinaryOp::Shl => TirBinOp::Shl,
        BinaryOp::Shr => TirBinOp::Shr,
        BinaryOp::UShr => TirBinOp::Ushr,
        BinaryOp::Instanceof | BinaryOp::In => return None,
    })
}

fn is_comparison(op: TirBinOp) -> bool {
    matches!(
        op,
        TirBinOp::Eq | TirBinOp::Ne | TirBinOp::Lt | TirBinOp::Le | TirBinOp::Gt | TirBinOp::Ge
    )
}

/// The same rule the TIR verifier's `check_binary` applies, checked here so a
/// failing case degrades to a placeholder instead of a verify error.
fn operands_coherent(op: TirBinOp, l: BackendTy, r: BackendTy, result: BackendTy) -> bool {
    let scalar = |t: BackendTy| {
        matches!(
            t,
            BackendTy::Int | BackendTy::Float | BackendTy::Bool | BackendTy::Str
        )
    };
    if !scalar(l) || l != r {
        return false;
    }
    if is_comparison(op) {
        return result == BackendTy::Bool;
    }
    let expected = if op == TirBinOp::Div && l == BackendTy::Int { BackendTy::Float } else { l };
    result == expected
}
