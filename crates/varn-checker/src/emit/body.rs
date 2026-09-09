//! Function bodies.
//!
//! Lowers every statement and expression the AST can hold. A form with no
//! precise TIR node — an iterator-protocol `for…of`, a Rest pattern, a host
//! construct — still produces real nodes (a `Loop`, a `MethodCall`, a `Cast`)
//! typed `Dynamic(Unannotated)`, never a half-built node the verifier cannot
//! check. `placeholder()` is that opaque-but-well-formed fallback.

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
    BackendTy, ClassId, ClassInfo, DynReason, EnumId, EnumInfo, LocalId, Resolution, Signature,
    SigId, Span,
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
    /// Named-argument layout by call-expression id.
    pub call_mappings: &'a FxHashMap<AstId, Vec<Option<usize>>>,
    /// `recv.m(..)` call ids the checker resolved to an extension function.
    pub ext_calls: &'a FxHashMap<u32, Rc<str>>,
    /// `recv.p` reads resolved to an extension getter.
    pub ext_members: &'a FxHashMap<u32, Rc<str>>,
    /// `recv.p = v` writes resolved to an extension setter.
    pub ext_set_members: &'a FxHashMap<u32, Rc<str>>,
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
    /// Set inside an enum method: `this` is typed as this enum, so a bare
    /// variant pattern (`Circle(r)`) in `match (this)` resolves.
    this_enum: Option<EnumId>,
    /// Extension method: `this` is param 0, not a receiver frame.
    /// This emitter is the module top level: a `let x` whose name is a module
    /// global becomes a store to that global slot, not a `<module>` local, so
    /// the other functions in the module (which see it as a global) agree.
    top_level: bool,
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
    /// `using x = …` bindings awaiting disposal, one frame per open block.
    /// `lower_block` appends `x.dispose()` for each (last-in first-out) as the
    /// block falls through.
    disposables: Vec<Vec<TirExpr>>,
}

enum ClosureBody<'a> {
    Expr(&'a Expr),
    Stmt(&'a Stmt),
}

/// Insert `fin` before every `Return` / `Break` / `Continue` that would leave
/// this statement list, recursing into nested `If` / `Loop` / `Try` bodies —
/// but NOT into a nested loop for `Break`/`Continue`, which stay inside it.
fn splice_finally_before_exits(stmts: Vec<TirStmt>, fin: &[TirStmt]) -> Vec<TirStmt> {
    splice_finally_impl(stmts, fin, false)
}

/// As `splice_finally_before_exits`, but also runs `fin` before a `Throw` that
/// would escape — used for a `catch` body, whose re-throw propagates past the
/// `finally`. Never used on the guarded `body`: a `throw` there is caught by
/// this try's own landing pad, and `finally` runs after the catch, not before.
fn splice_finally_before_exits_and_throw(stmts: Vec<TirStmt>, fin: &[TirStmt]) -> Vec<TirStmt> {
    splice_finally_impl(stmts, fin, true)
}

fn splice_finally_impl(stmts: Vec<TirStmt>, fin: &[TirStmt], on_throw: bool) -> Vec<TirStmt> {
    let mut out = Vec::with_capacity(stmts.len());
    for s in stmts {
        match s {
            TirStmt::Return(_) | TirStmt::Break | TirStmt::Continue => {
                out.extend(fin.iter().cloned());
                out.push(s);
            }
            TirStmt::Throw(_) if on_throw => {
                out.extend(fin.iter().cloned());
                out.push(s);
            }
            TirStmt::If { cond, then_body, else_body } => out.push(TirStmt::If {
                cond,
                then_body: splice_finally_impl(then_body, fin, on_throw),
                else_body: splice_finally_impl(else_body, fin, on_throw),
            }),
            TirStmt::Loop { cond, body } => {
                // `break` / `continue` here belong to this inner loop; only a
                // `Return` escapes the guarded region.
                out.push(TirStmt::Loop {
                    cond,
                    body: splice_returns_only(body, fin),
                });
            }
            TirStmt::Try { body, catch_local, catch_body } => out.push(TirStmt::Try {
                body: splice_finally_impl(body, fin, false),
                catch_local,
                catch_body: splice_finally_impl(catch_body, fin, on_throw),
            }),
            other => out.push(other),
        }
    }
    out
}

fn splice_returns_only(stmts: Vec<TirStmt>, fin: &[TirStmt]) -> Vec<TirStmt> {
    let mut out = Vec::with_capacity(stmts.len());
    for s in stmts {
        match s {
            TirStmt::Return(_) => {
                out.extend(fin.iter().cloned());
                out.push(s);
            }
            TirStmt::If { cond, then_body, else_body } => out.push(TirStmt::If {
                cond,
                then_body: splice_returns_only(then_body, fin),
                else_body: splice_returns_only(else_body, fin),
            }),
            TirStmt::Loop { cond, body } => {
                out.push(TirStmt::Loop { cond, body: splice_returns_only(body, fin) })
            }
            TirStmt::Try { body, catch_local, catch_body } => out.push(TirStmt::Try {
                body: splice_returns_only(body, fin),
                catch_local,
                catch_body: splice_returns_only(catch_body, fin),
            }),
            other => out.push(other),
        }
    }
    out
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
            this_enum: None,
            top_level: false,
            outer_names: FxHashSet::default(),
            captures: Vec::new(),
            pending: Vec::new(),
            disposables: Vec::new(),
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

    pub fn with_this_enum(mut self, enum_id: EnumId) -> Self {
        self.this_enum = Some(enum_id);
        self
    }


    pub fn as_top_level(mut self) -> Self {
        self.top_level = true;
        self
    }

    /// Lower a single expression that sits outside a statement — a decorator,
    /// an `extends` clause, a static-field initializer. Any hoisted temporary
    /// it produced is returned alongside; the caller emits those first.
    pub fn lower_outer_expr(&mut self, e: &Expr) -> (Vec<TirStmt>, TirExpr) {
        let x = self.lower_expr(e);
        (std::mem::take(&mut self.pending), x)
    }

    pub fn lower_expression(&mut self, e: &Expr) -> TirExpr {
        self.lower_expr(e)
    }

    pub fn take_pending(&mut self) -> Vec<TirStmt> {
        std::mem::take(&mut self.pending)
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
        self.disposables.push(Vec::new());
        let mut out = Vec::new();
        for s in stmts {
            out.extend(self.lower_stmt(s));
        }
        // `using` bindings dispose on the way out, most-recent first.
        for resource in self.disposables.pop().unwrap_or_default().into_iter().rev() {
            out.push(TirStmt::Expr(TirExpr {
                kind: TirExprKind::MethodCall {
                    recv: Box::new(resource),
                    name: Rc::from("dispose"),
                    args: vec![],
                },
                ty: BackendTy::Void,
                res: Resolution::ByName {
                    name: Rc::from("dispose"),
                    why: DynReason::Unannotated,
                },
                span: Span::EMPTY,
            }));
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
            StmtKind::ForIn { left, right, body, .. } => self.lower_for_in(left, right, body),

            StmtKind::Try { block, catches, finally } => {
                self.lower_try(block, catches, finally.as_deref())
            }

            StmtKind::Switch { discriminant, cases } => {
                self.lower_switch(discriminant, cases)
            }

            // `using x = …` disposes at scope end; the binding itself lowers
            // like a `let`, the disposal is a runtime concern.
            StmtKind::Using { declarations, .. } => {
                let mut out = Vec::new();
                for d in declarations {
                    if let Pattern::Identifier { name, .. } = &d.id {
                        let init = d.init.as_ref().map(|e| self.lower_expr(e));
                        let ty = init
                            .as_ref()
                            .map(|e| e.ty)
                            .unwrap_or(BackendTy::Dynamic(DynReason::Unannotated));
                        let local = self.bind_local(name.clone(), ty);
                        out.extend(std::mem::take(&mut self.pending));
                        out.push(TirStmt::Let { local, ty, init });
                        if let Some(frame) = self.disposables.last_mut() {
                            frame.push(TirExpr {
                                kind: TirExprKind::Var,
                                ty,
                                res: Resolution::Local(local),
                                span: Span::EMPTY,
                            });
                        }
                    }
                }
                out
            }

            // A labeled statement — the label is only meaningful to a
            // `break`/`continue` targeting it, which we do not model yet; run
            // the body.
            StmtKind::Labeled { body, .. } => self.lower_stmt_as_block(body),
        }
    }

    /// `try { } catch (e) { } finally { }`. `finally` has no TIR node — its
    /// statements are appended after the `Try` (correct for straight-line and
    /// caught paths, not for a `return`/`throw` that escapes the try).
    /// The class names a `catch (e: T)` / `catch (e: A | B)` filter tests.
    fn catch_type_names(&self, t: &varn_core::ast::TypeNode) -> Vec<Rc<str>> {
        use varn_core::TypeKind;
        match &t.kind {
            TypeKind::Named(n, _) => vec![Rc::from(n.as_str())],
            TypeKind::Union(items) | TypeKind::Intersection(items) => {
                items.iter().flat_map(|x| self.catch_type_names(x)).collect()
            }
            _ => vec![],
        }
    }

    /// `e instanceof <name>` — a `TypeTest` for a module class, else a dynamic
    /// `instanceof` against the global of that name (builtins: `TypeError` …).
    fn instance_of_name(&self, value: TirExpr, name: &str) -> TirExpr {
        let span = value.span;
        if let Some(class) = self.m.names.class_id(name) {
            return TirExpr {
                kind: TirExprKind::TypeTest { value: Box::new(value), class },
                ty: BackendTy::Bool,
                res: Resolution::None,
                span,
            };
        }
        let rhs = TirExpr {
            kind: TirExprKind::Var,
            ty: BackendTy::Dynamic(DynReason::Unannotated),
            res: Resolution::ByName { name: Rc::from(name), why: DynReason::Unannotated },
            span,
        };
        TirExpr {
            kind: TirExprKind::Binary {
                op: TirBinOp::Instanceof,
                lhs: Box::new(value),
                rhs: Box::new(rhs),
            },
            ty: BackendTy::Bool,
            res: Resolution::None,
            span,
        }
    }

    fn lower_try(
        &mut self,
        block: &Stmt,
        catches: &[varn_core::ast::CatchClause],
        finally: Option<&Stmt>,
    ) -> Vec<TirStmt> {
        let body = self.lower_stmt_as_block(block);
        let dyn_ty = BackendTy::Dynamic(DynReason::Unannotated);
        // One landing local holds the thrown value; typed clauses dispatch on
        // `e instanceof T`, an untyped clause is the catch-all, and if none
        // catches the value it is re-thrown.
        self.scopes.push(FxHashMap::default());
        let catch_local = self.bind_local(Rc::from("<catch>"), dyn_ty);
        let e_var = |span: Span| TirExpr {
            kind: TirExprKind::Var,
            ty: dyn_ty,
            res: Resolution::Local(catch_local),
            span,
        };
        let lower_clause = |this: &mut Self, c: &varn_core::ast::CatchClause| -> Vec<TirStmt> {
            this.scopes.push(FxHashMap::default());
            let mut out = Vec::new();
            if let Some(Pattern::Identifier { name, .. }) = &c.param {
                if name.as_ref() != "<catch>" {
                    let alias = this.bind_local(name.clone(), dyn_ty);
                    out.push(TirStmt::Let {
                        local: alias,
                        ty: dyn_ty,
                        init: Some(e_var(Span::EMPTY)),
                    });
                }
            }
            out.extend(this.lower_stmt_as_block(&c.body));
            this.scopes.pop();
            out
        };
        let typed: Vec<&varn_core::ast::CatchClause> =
            catches.iter().filter(|c| c.type_ann.is_some()).collect();
        let catch_all = catches.iter().find(|c| c.type_ann.is_none());
        let mut chain: Vec<TirStmt> = match catch_all {
            Some(c) => lower_clause(self, c),
            None => vec![TirStmt::Throw(e_var(Span::EMPTY))],
        };
        for c in typed.iter().rev() {
            let names = self.catch_type_names(c.type_ann.as_ref().unwrap());
            let cond = names
                .into_iter()
                .map(|n| self.instance_of_name(e_var(Span::EMPTY), &n))
                .reduce(|a, b| TirExpr {
                    kind: TirExprKind::Select {
                        cond: Box::new(a),
                        then_val: Box::new(bool_lit(true)),
                        else_val: Box::new(b),
                    },
                    ty: BackendTy::Bool,
                    res: Resolution::None,
                    span: Span::EMPTY,
                })
                .unwrap_or_else(|| bool_lit(true));
            let then_body = lower_clause(self, c);
            chain = vec![TirStmt::If { cond, then_body, else_body: std::mem::take(&mut chain) }];
        }
        self.scopes.pop();
        let catch_body = chain;
        // `finally` has no TIR node: lower it once, then run it on the normal
        // fall-through AND before every early exit inside the guarded region.
        let fin: Vec<TirStmt> = finally
            .map(|f| self.lower_stmt_as_block(f))
            .unwrap_or_default();
        let (body, catch_body) = if fin.is_empty() {
            (body, catch_body)
        } else {
            (
                splice_finally_before_exits(body, &fin),
                splice_finally_before_exits_and_throw(catch_body, &fin),
            )
        };
        let mut out = vec![TirStmt::Try { body, catch_local, catch_body }];
        out.extend(fin);
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

    /// `for (x of iterable) body`. An `Array` iterable becomes the index
    /// desugar; anything else goes through the iterator protocol
    /// (`.iterator()` / `.next()`), all by-name.
    /// `for (k in obj)` — iterate the object's string keys. Lowered as a
    /// for-of over `ObjectKeys(obj)`, which is a `str[]`.
    fn lower_for_in(&mut self, left: &Pattern, right: &Expr, body: &Stmt) -> Vec<TirStmt> {
        let obj = self.lower_expr(right);
        let mut out = std::mem::take(&mut self.pending);
        let s = self.tt.intern(BackendTy::Str);
        let keys_ty = BackendTy::Array(s);
        let keys = TirExpr {
            kind: TirExprKind::ObjectKeys { operand: Box::new(obj) },
            ty: keys_ty,
            res: Resolution::None,
            span: Span::EMPTY,
        };
        let keys = self.hoist(keys);
        out.extend(std::mem::take(&mut self.pending));
        let Pattern::Identifier { name, .. } = left else {
            return self.lower_for_of_protocol(left, keys, out, body);
        };
        let name = name.clone();
        out.extend(self.for_of_over_array(&name, keys, BackendTy::Str, body));
        out
    }

    fn lower_for_of(&mut self, left: &Pattern, right: &Expr, body: &Stmt) -> Vec<TirStmt> {
        // `for (i of a..b)` — a plain integer loop, `..=` bumping the bound.
        if let (ExprKind::Range { start, end, inclusive }, Pattern::Identifier { name, .. }) =
            (&right.kind, left)
        {
            let name = name.clone();
            let lo = self.lower_expr(start);
            let mut hi = self.lower_expr(end);
            if *inclusive {
                hi = TirExpr {
                    kind: TirExprKind::Binary {
                        op: TirBinOp::Add,
                        lhs: Box::new(hi),
                        rhs: Box::new(int_lit(1)),
                    },
                    ty: BackendTy::Int,
                    res: Resolution::None,
                    span: Span::EMPTY,
                };
            }
            let mut out = std::mem::take(&mut self.pending);
            let hi = self.hoist(hi);
            out.extend(std::mem::take(&mut self.pending));
            let i = self.bind_local(name, BackendTy::Int);
            let ivar = || TirExpr {
                kind: TirExprKind::Var,
                ty: BackendTy::Int,
                res: Resolution::Local(i),
                span: Span::EMPTY,
            };
            out.push(TirStmt::Let { local: i, ty: BackendTy::Int, init: Some(lo) });
            let cond = TirExpr {
                kind: TirExprKind::Binary {
                    op: TirBinOp::Lt,
                    lhs: Box::new(ivar()),
                    rhs: Box::new(hi),
                },
                ty: BackendTy::Bool,
                res: Resolution::None,
                span: Span::EMPTY,
            };
            let mut loop_body =
                vec![TirStmt::If { cond, then_body: vec![], else_body: vec![TirStmt::Break] }];
            loop_body.extend(self.lower_stmt_as_block(body));
            loop_body.push(TirStmt::Expr(TirExpr {
                kind: TirExprKind::Assign {
                    target: Box::new(ivar()),
                    value: Box::new(TirExpr {
                        kind: TirExprKind::Binary {
                            op: TirBinOp::Add,
                            lhs: Box::new(ivar()),
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
            return out;
        }

        let iter = self.lower_expr(right);
        let mut out = std::mem::take(&mut self.pending);

        let Pattern::Identifier { name, .. } = left else {
            return self.lower_for_of_protocol(left, iter, out, body);
        };
        // A statically-typed array indexes directly. So does a `Dynamic`
        // subject — `for (x of bucket)` where `bucket` came off an index
        // signature is the overwhelmingly common case, and the VM's
        // `length` / `[i]` work on any runtime array. Only a value with a
        // known non-array iterable type takes the `.iterator()` protocol.
        let elem_ty = match iter.ty.non_nullable(self.tt) {
            BackendTy::Array(el) => self.tt.get(el),
            BackendTy::Dynamic(_) => BackendTy::Dynamic(DynReason::Unannotated),
            _ => return self.lower_for_of_protocol(left, iter, out, body),
        };
        let arr = self.hoist(iter);
        out.extend(std::mem::take(&mut self.pending));
        out.extend(self.for_of_over_array(name, arr, elem_ty, body));
        out
    }

    /// The C-style index loop shared by array `for…of` and `for…in`:
    /// `let i = 0; loop { if !(i < arr.length) break; let <name> = arr[i];
    /// body; i = i + 1 }`.
    fn for_of_over_array(
        &mut self,
        name: &Rc<str>,
        arr: TirExpr,
        elem_ty: BackendTy,
        body: &Stmt,
    ) -> Vec<TirStmt> {
        let mut out = Vec::new();
        let idx = self.fresh_local(BackendTy::Int);
        out.push(TirStmt::Let { local: idx, ty: BackendTy::Int, init: Some(int_lit(0)) });
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

    /// Generic `for…of` / `for…in`: `let it = src.iterator(); loop { let step =
    /// it.next(); if step.done break; <bind pat from step.value>; body }`. All
    /// members resolve by name; the values are `Dynamic`.
    fn lower_for_of_protocol(
        &mut self,
        left: &Pattern,
        src: TirExpr,
        mut out: Vec<TirStmt>,
        body: &Stmt,
    ) -> Vec<TirStmt> {
        let dyn_ty = BackendTy::Dynamic(DynReason::Unannotated);
        let by_name = |n: &str| Resolution::ByName { name: Rc::from(n), why: DynReason::Unannotated };

        let it = self.fresh_local(dyn_ty);
        out.push(TirStmt::Let {
            local: it,
            ty: dyn_ty,
            init: Some(TirExpr {
                kind: TirExprKind::MethodCall {
                    recv: Box::new(src),
                    name: Rc::from("iterator"),
                    args: vec![],
                },
                ty: dyn_ty,
                res: by_name("iterator"),
                span: Span::EMPTY,
            }),
        });
        let it_var = || TirExpr {
            kind: TirExprKind::Var,
            ty: dyn_ty,
            res: Resolution::Local(it),
            span: Span::EMPTY,
        };

        let step = self.fresh_local(dyn_ty);
        let step_var = || TirExpr {
            kind: TirExprKind::Var,
            ty: dyn_ty,
            res: Resolution::Local(step),
            span: Span::EMPTY,
        };
        let field = |recv: TirExpr, n: &str| TirExpr {
            kind: TirExprKind::Field { object: Box::new(recv), name: Rc::from(n) },
            ty: BackendTy::Dynamic(DynReason::Unannotated),
            res: Resolution::ByName { name: Rc::from(n), why: DynReason::Unannotated },
            span: Span::EMPTY,
        };

        let mut loop_body = vec![
            TirStmt::Let {
                local: step,
                ty: dyn_ty,
                init: Some(TirExpr {
                    kind: TirExprKind::MethodCall {
                        recv: Box::new(it_var()),
                        name: Rc::from("next"),
                        args: vec![],
                    },
                    ty: dyn_ty,
                    res: by_name("next"),
                    span: Span::EMPTY,
                }),
            },
            TirStmt::If {
                cond: field(step_var(), "done"),
                then_body: vec![TirStmt::Break],
                else_body: vec![],
            },
        ];
        let value = field(step_var(), "value");
        self.bind_pattern(left, value, &mut loop_body);
        loop_body.extend(self.lower_stmt_as_block(body));

        out.push(TirStmt::Loop { cond: bool_lit(true), body: loop_body });
        out
    }

    /// `match subject { … }` -> hoist the subject, then a chain of `If`.
    /// `dest` says what each arm does with its value: nothing (statement
    /// position), `return` it, or assign it to a local (`let x = match …`). A
    /// pattern with no TIR shape (Record / Sequence) becomes a dead branch
    /// (`false` condition) but its body still lowers.
    fn lower_match(&mut self, subject: &Expr, cases: &[MatchCase], dest: MatchDest) -> Vec<TirStmt> {
        let subj = self.lower_expr(subject);
        let mut out = std::mem::take(&mut self.pending);
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
        let (cond, bindings) = self.match_pattern(s, &case.pattern);
        // A guard (`P if expr => …`) reads the pattern's bindings, so it must
        // run AFTER they are declared — inside the matched branch, not folded
        // into the entry test. A false guard falls through to the later cases,
        // which is why they are re-tried here as well as on a pattern miss.
        let guard = case.guard.as_ref().map(|g| {
            let gexpr = self.lower_expr(g);
            let pending = std::mem::take(&mut self.pending);
            (pending, gexpr)
        });
        let mut then_body: Vec<TirStmt> = Vec::new();
        // The arm's value expression, then what `dest` does with it.
        let value = match &case.body {
            MatchBody::Expr(e) => self.lower_expr(e),
            MatchBody::Block(stmt) => {
                // A block-bodied arm in value position is not lowered yet;
                // run it for effect and yield null.
                then_body.extend(self.lower_stmt_as_block(stmt));
                TirExpr {
                    kind: TirExprKind::NullLit,
                    ty: BackendTy::Dynamic(DynReason::Unannotated),
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
        match guard {
            None => {
                let mut m = bindings;
                m.extend(then_body);
                vec![TirStmt::If { cond, then_body: m, else_body }]
            }
            Some((gpending, gexpr)) => {
                // pattern matched: declare its bindings, evaluate the guard;
                // on true run the arm, on false (or a pattern miss) fall to
                // the later cases.
                let mut matched = bindings;
                matched.extend(gpending);
                matched.push(TirStmt::If {
                    cond: gexpr,
                    then_body,
                    else_body: else_body.clone(),
                });
                vec![TirStmt::If { cond, then_body: matched, else_body }]
            }
        }
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
            // Record / Sequence patterns have no TIR shape yet: a dead branch.
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
        // An imported enum has no local `EnumInfo`; match on the variant's
        // runtime name instead, and pull payload fields by ordinal.
        let Some(eid) = eid else {
            return self.match_variant_by_name(s, variant_name, bindings);
        };
        let Some(info) = self.m.enums.get(eid.0 as usize) else {
            return self.match_variant_by_name(s, variant_name, bindings);
        };
        let Some(variant) = info.variants.iter().find(|v| v.name.as_ref() == variant_name) else {
            return self.match_variant_by_name(s, variant_name, bindings);
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
            let fty = payload.get(i).copied().unwrap_or(BackendTy::Dynamic(DynReason::Unannotated));
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

    /// `match (opt) { Some(v) => … }` where `opt`'s enum is imported (no local
    /// `EnumInfo`): test the runtime `__variant_name__`, read payloads by the
    /// `valueN` ordinal accessor.
    fn match_variant_by_name(
        &mut self,
        s: &TirExpr,
        variant_name: &str,
        bindings: &[MatchBinding],
    ) -> (TirExpr, Vec<TirStmt>) {
        let dyn_ty = BackendTy::Dynamic(DynReason::Unannotated);
        let by_name = |n: &str| Resolution::ByName { name: Rc::from(n), why: DynReason::Unannotated };
        let field = |recv: TirExpr, name: &str, ty: BackendTy| TirExpr {
            kind: TirExprKind::Field { object: Box::new(recv), name: Rc::from(name) },
            ty,
            res: by_name(name),
            span: s.span,
        };
        let cond = TirExpr {
            kind: TirExprKind::Binary {
                op: TirBinOp::Eq,
                lhs: Box::new(field(s.clone(), "__variant_name__", BackendTy::Str)),
                rhs: Box::new(TirExpr {
                    kind: TirExprKind::StrLit(Rc::from(variant_name)),
                    ty: BackendTy::Str,
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
            let init = field(s.clone(), &format!("value{i}"), dyn_ty);
            let local = self.bind_local(b.name.clone(), dyn_ty);
            binds.push(TirStmt::Let { local, ty: dyn_ty, init: Some(init) });
        }
        (cond, binds)
    }

    fn lower_decl_stmt(&mut self, decl: &varn_core::ast::Decl) -> Vec<TirStmt> {
        use varn_core::ast::{Decl, ExportDecl};
        let unwrapped = match decl {
            Decl::Export(ExportDecl::Decl { declaration, .. }) => declaration.as_ref(),
            other => other,
        };
        // A named function declared inside a body is a local bound to a closure
        // over the enclosing frame. (Top-level function declarations never reach
        // here — they are free functions.)
        if let Decl::Function(f) = unwrapped {
            let dyn_ty = BackendTy::Dynamic(DynReason::Unannotated);
            let local = self.bind_local(f.id.clone(), dyn_ty);
            let closure = self.lower_closure(
                &f.params,
                ClosureBody::Stmt(&f.body),
                f.modifiers.is_async,
                f.modifiers.is_generator,
                dyn_ty,
                Span::EMPTY,
            );
            return vec![TirStmt::Let { local, ty: dyn_ty, init: Some(closure) }];
        }
        let v = match decl {
            Decl::Variable(v) => v,
            Decl::Export(ExportDecl::Decl { declaration, .. }) => match declaration.as_ref() {
                Decl::Variable(v) => v,
                _ => return vec![], // nested class/enum: handled at module level
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

                    // `const f = () => { … f() … }` — the closure refers to
                    // itself. Bind the name before lowering the initializer so
                    // the self-reference is a capture of this local, not a null
                    // global. (A top-level global is already reachable by name.)
                    let prebound = {
                        let is_closure = matches!(
                            d.init.as_ref().map(|e| &e.kind),
                            Some(ExprKind::Arrow { .. } | ExprKind::Function { .. })
                        );
                        let is_global =
                            self.top_level && self.m.globals.contains_key(name.as_ref());
                        if is_closure && !is_global {
                            Some(self.bind_local(
                                name.clone(),
                                BackendTy::Dynamic(DynReason::Unannotated),
                            ))
                        } else {
                            None
                        }
                    };

                    let init = d.init.as_ref().map(|e| self.lower_expr(e));
                    // The declared annotation wins over the initializer's type
                    // — `let x: float = 1` is a `float` binding, and a typed op
                    // that later reads `x` must not pick the `int` opcode.
                    let ty = d
                        .type_ann
                        .as_ref()
                        .map(|t| {
                            let resolved =
                                crate::binder::resolve_type_node(t, None);
                            lower_type(&resolved, self.tt, self.m.names)
                        })
                        .filter(|t| !matches!(t, BackendTy::Dynamic(_)))
                        .or_else(|| init.as_ref().map(|e| e.ty))
                        .unwrap_or(BackendTy::Dynamic(DynReason::Unannotated));
                    out.extend(std::mem::take(&mut self.pending));

                    // A module-level binding that is a module global: store it
                    // to the slot, not to a `<module>` local.
                    if self.top_level && prebound.is_none() {
                        if let Some(&slot) = self.m.globals.get(name.as_ref()) {
                            let value = init.unwrap_or_else(|| TirExpr {
                                kind: TirExprKind::NullLit,
                                ty,
                                res: Resolution::None,
                                span: Span::EMPTY,
                            });
                            let target = TirExpr {
                                kind: TirExprKind::Var,
                                ty,
                                res: Resolution::GlobalSlot(slot),
                                span: Span::EMPTY,
                            };
                            out.push(TirStmt::Expr(TirExpr {
                                kind: TirExprKind::Assign {
                                    target: Box::new(target),
                                    value: Box::new(value),
                                },
                                ty: BackendTy::Void,
                                res: Resolution::None,
                                span: Span::EMPTY,
                            }));
                            continue;
                        }
                    }

                    let local = prebound.unwrap_or_else(|| self.bind_local(name.clone(), ty));
                    out.push(TirStmt::Let { local, ty, init });
                }
                // Destructuring: `let {a,b} = obj` / `let [x,y] = arr`.
                pat => {
                    let src = match &d.init {
                        Some(init) => self.lower_expr(init),
                        None => placeholder(DynReason::Unannotated),
                    };
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
            // `x = default` — a null (omitted) argument falls back to the
            // default: `if (x == null) x = <default>`.
            if let Some(def) = &p.default {
                let pvar = || TirExpr {
                    kind: TirExprKind::Var,
                    ty: BackendTy::Dynamic(DynReason::Unannotated),
                    res: Resolution::Param(i as u32),
                    span: Span::EMPTY,
                };
                let is_null = TirExpr {
                    kind: TirExprKind::Unary {
                        op: TirUnOp::IsNull,
                        operand: Box::new(pvar()),
                    },
                    ty: BackendTy::Bool,
                    res: Resolution::None,
                    span: Span::EMPTY,
                };
                let value = self.lower_expr(def);
                let assign = TirExpr {
                    kind: TirExprKind::Assign {
                        target: Box::new(pvar()),
                        value: Box::new(value),
                    },
                    ty: BackendTy::Dynamic(DynReason::Unannotated),
                    res: Resolution::None,
                    span: Span::EMPTY,
                };
                out.append(&mut self.pending);
                out.push(TirStmt::If {
                    cond: is_null,
                    then_body: vec![TirStmt::Expr(assign)],
                    else_body: vec![],
                });
            }

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
            Pattern::Object { properties, rest, .. } => {
                for prop in properties {
                    let field = self.field_access(
                        src.clone(),
                        prop.key.clone(),
                        BackendTy::Dynamic(DynReason::Unannotated),
                        src.span,
                    );
                    self.bind_pattern(&prop.value, field, out);
                }
                if let Some(rest_pat) = rest {
                    let skip: Vec<Rc<str>> = properties.iter().map(|p| p.key.clone()).collect();
                    let rest_obj = TirExpr {
                        kind: TirExprKind::ObjectRest {
                            object: Box::new(src.clone()),
                            skip_keys: skip,
                        },
                        ty: BackendTy::Dynamic(DynReason::Unannotated),
                        res: Resolution::None,
                        span: src.span,
                    };
                    self.bind_pattern(rest_pat, rest_obj, out);
                }
            }
            Pattern::Array { elements, rest, .. } => {
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
                // `[a, b, ...rest]` — the tail from index `elements.len()`.
                if let Some(rest_pat) = rest {
                    let tail = TirExpr {
                        kind: TirExprKind::MethodCall {
                            recv: Box::new(src.clone()),
                            name: Rc::from("slice"),
                            args: vec![TirArg::Expr(int_lit(elements.len() as i64))],
                        },
                        ty: src.ty,
                        res: Resolution::ByName {
                            name: Rc::from("slice"),
                            why: DynReason::Unannotated,
                        },
                        span: src.span,
                    };
                    self.bind_pattern(rest_pat, tail, out);
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
            // `...rest` — no slice primitive yet, so the rest binding gets the
            // whole source (a coarse but well-formed lowering).
            Pattern::Rest { argument, .. } => self.bind_pattern(argument, src, out),
        }
    }

    /// A condition the verifier requires to be `Bool`. A non-Bool, non-Dynamic
    /// value (a truthy `int`, a nullable) is wrapped in a `Cast` — the runtime
    /// applies the truthiness rule.
    fn lower_cond(&mut self, e: &Expr) -> TirExpr {
        let lowered = self.lower_expr(e);
        match lowered.ty {
            BackendTy::Bool | BackendTy::Dynamic(_) => lowered,
            _ => self.cast_to(lowered, BackendTy::Bool),
        }
    }

    /// Wrap `e` in a `Cast` to `ty`. The verifier trusts a `Cast`; the backend
    /// performs (or elides) the conversion.
    fn cast_to(&self, e: TirExpr, ty: BackendTy) -> TirExpr {
        if e.ty == ty {
            return e;
        }
        let span = e.span;
        TirExpr {
            kind: TirExprKind::Cast { operand: Box::new(e) },
            ty,
            res: Resolution::None,
            span,
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
                    .this_enum
                    .map(BackendTy::Enum)
                    .or_else(|| self.this_class.map(BackendTy::Class))
                    .unwrap_or(BackendTy::Dynamic(DynReason::Unannotated));
                return TirExpr { kind: TirExprKind::Var, ty: this_ty, res: Resolution::None, span };
            }

            ExprKind::Member { object, property, computed, optional } => {
                return self.lower_member(object, property, *computed, *optional, ty, span)
            }

            ExprKind::Logical { op, left, right } => {
                return self.lower_logical(*op, left, right, ty, span)
            }

            ExprKind::Template { parts } => return self.lower_template(parts, span),

            // `match` in expression position: a result temp, the If-chain as
            // pending statements assigning it, then a read of the temp.
            ExprKind::Match { subject, cases } => {
                let result = self.fresh_local(ty);
                self.pending.push(TirStmt::Let { local: result, ty, init: None });
                let chain = self.lower_match(subject, cases, MatchDest::Assign(result));
                self.pending.extend(chain);
                return TirExpr {
                    kind: TirExprKind::Var,
                    ty,
                    res: Resolution::Local(result),
                    span,
                };
            }

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
                    ty: BackendTy::Dynamic(DynReason::Unannotated),
                    res: Resolution::None,
                    span,
                };
            }

            ExprKind::Call { callee, args, optional: _, type_args: _ } => {
                return self.lower_call(e.id, callee, args, ty, span)
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
            ExprKind::Record { properties } => {
                let fields = properties
                    .iter()
                    .filter_map(|p| match p {
                        ObjectProp::Property { key, value, .. } => {
                            Some((prop_key_name(key)?, self.lower_expr(value)))
                        }
                        _ => None,
                    })
                    .collect();
                return TirExpr {
                    kind: TirExprKind::RecordLit { fields },
                    ty,
                    res: Resolution::None,
                    span,
                };
            }
            ExprKind::Object { properties } => {
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
            ExprKind::New { callee, args, .. } => return self.lower_new(e.id, callee, args, ty, span),

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
                    return TirExpr {
                        kind: TirExprKind::NullLit,
                        ty: BackendTy::Void,
                        res: Resolution::None,
                        span,
                    };
                };
                for e in lead {
                    let te = self.lower_expr(e);
                    self.pending.push(TirStmt::Expr(te));
                }
                return self.lower_expr(last);
            }
            // `x |> f` -> `f(x)`.
            ExprKind::Pipeline { left, right } => {
                // `x |> f(_, y)` — substitute `x` for each `_` in the call's
                // arguments. `x |> f` (no call) — `f(x)`.
                if let ExprKind::Call { callee, args, .. } = &right.kind {
                    let has_placeholder = args.iter().any(|a| {
                        matches!(
                            a,
                            Arg::Positional(e) | Arg::Named { value: e, .. }
                                if matches!(&e.kind, ExprKind::Identifier { name } if name.as_ref() == "_")
                        )
                    });
                    if has_placeholder {
                        let lv = self.lower_expr(left);
                        let piped = self.hoist(lv);
                        let c = self.lower_expr(callee);
                        let targs: Vec<TirArg> = args
                            .iter()
                            .map(|a| match a {
                                Arg::Positional(e)
                                | Arg::Named { value: e, .. }
                                    if matches!(&e.kind, ExprKind::Identifier { name } if name.as_ref() == "_") =>
                                {
                                    TirArg::Expr(piped.clone())
                                }
                                other => self.lower_arg(other),
                            })
                            .collect();
                        return TirExpr {
                            kind: TirExprKind::Call { callee: Box::new(c), args: targs },
                            ty,
                            res: Resolution::ByName {
                                name: Rc::from("<pipeline>"),
                                why: DynReason::Unannotated,
                            },
                            span,
                        };
                    }
                }
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
            // `recv.p = v` resolved to an extension setter: `__extset(recv, v)`.
            ExprKind::Assign { op, target, value }
                if matches!(assign_bin_op(*op), Ok(None))
                    && matches!(&target.kind, ExprKind::Member { .. })
                    && self.m.ext_set_members.contains_key(&target.range.start.offset) =>
            {
                let ExprKind::Member { object, .. } = &target.kind else {
                    unreachable!()
                };
                let mangled =
                    self.m.ext_set_members[&target.range.start.offset].clone();
                let recv = self.lower_expr(object);
                let v = self.lower_expr(value);
                return TirExpr {
                    kind: TirExprKind::ExtensionCall {
                        func: mangled,
                        recv: Box::new(recv),
                        args: vec![TirArg::Expr(v)],
                    },
                    ty,
                    res: Resolution::None,
                    span,
                };
            }
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
                    Ok(Some(bop)) => {
                        let (lhs, rhs, nty) =
                            self.coerce_binary_operands(bop, t.clone(), v, t.ty);
                        TirExpr {
                            kind: TirExprKind::Binary {
                                op: bop,
                                lhs: Box::new(lhs),
                                rhs: Box::new(rhs),
                            },
                            ty: nty,
                            res: Resolution::None,
                            span,
                        }
                    }
                    // `&&=` / `||=` / `??=` — short-circuit against the current
                    // value: `t = t ? v : t`, `t = t ? t : v`, `t = t ?? v`.
                    Err(()) => {
                        use varn_core::ast::operators::AssignOp as A;
                        let cond = match op {
                            A::NullishAssign => TirExpr {
                                kind: TirExprKind::Unary {
                                    op: TirUnOp::IsNull,
                                    operand: Box::new(t.clone()),
                                },
                                ty: BackendTy::Bool,
                                res: Resolution::None,
                                span,
                            },
                            _ => self.cast_to(t.clone(), BackendTy::Bool),
                        };
                        let (then_val, else_val) = match op {
                            A::OrAssign => (t.clone(), v),
                            _ => (v, t.clone()), // AndAssign, NullishAssign
                        };
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
            ExprKind::Update { op, operand, prefix }
                if matches!(
                    operand.kind,
                    ExprKind::Identifier { .. } | ExprKind::Member { .. }
                ) && Self::is_pure(operand) =>
            {
                use varn_core::ast::operators::UpdateOp;
                let t = self.lower_expr(operand);
                let bop = match op {
                    UpdateOp::Increment => TirBinOp::Add,
                    UpdateOp::Decrement => TirBinOp::Sub,
                };
                let step = if t.ty == BackendTy::Float { self.cast_to(int_lit(1), BackendTy::Float) } else { int_lit(1) };
                // Postfix (`x++`) yields the value BEFORE the step; hoist it so
                // the assignment can still overwrite the target.
                let old = if *prefix { t.clone() } else { self.hoist(t.clone()) };
                let (lhs, rhs, nty) = self.coerce_binary_operands(bop, old.clone(), step, t.ty);
                let stepped = TirExpr {
                    kind: TirExprKind::Binary { op: bop, lhs: Box::new(lhs), rhs: Box::new(rhs) },
                    ty: nty,
                    res: Resolution::None,
                    span,
                };
                let assign = TirExpr {
                    kind: TirExprKind::Assign {
                        target: Box::new(t),
                        value: Box::new(stepped),
                    },
                    ty: nty,
                    res: Resolution::None,
                    span,
                };
                if *prefix {
                    return assign;
                }
                // `x++` — run the store for effect, evaluate to the old value.
                self.pending.push(TirStmt::Expr(assign));
                return old;
            }

            // `bigint` / `decimal` / regex literals have no TIR literal node:
            // the raw text as a Str, cast to the target type.
            ExprKind::DecimalLiteral { raw } => {
                let text: Rc<str> = Rc::from(raw.trim_end_matches('d'));
                Some(TirExprKind::DecimalLit(text))
            }
            ExprKind::BigIntLiteral { raw } => {
                let s = raw.trim_end_matches('n').replace('_', "");
                let n = if let Some(r) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
                    i128::from_str_radix(r, 16)
                } else if let Some(r) = s.strip_prefix("0o").or_else(|| s.strip_prefix("0O")) {
                    i128::from_str_radix(r, 8)
                } else if let Some(r) = s.strip_prefix("0b").or_else(|| s.strip_prefix("0B")) {
                    i128::from_str_radix(r, 2)
                } else {
                    s.parse()
                }
                .unwrap_or(0);
                Some(TirExprKind::BigIntLit(n))
            }
            ExprKind::RegexLiteral { pattern, flags } => {
                let s = TirExpr {
                    kind: TirExprKind::StrLit(Rc::from(format!("/{pattern}/{flags}"))),
                    ty: BackendTy::Str,
                    res: Resolution::None,
                    span,
                };
                return self.cast_to(s, ty);
            }

            // `spawn e` runs `e` and yields a task handle; `e` is lowered and
            // the node takes the checked (task) type.
            ExprKind::Spawn { argument } => {
                let inner = self.lower_expr(argument);
                return self.cast_to(inner, ty);
            }
            ExprKind::Range { start, end, inclusive } => {
                let s = self.lower_expr(start);
                let en = self.lower_expr(end);
                return TirExpr {
                    kind: TirExprKind::RangeLit {
                        start: Box::new(s),
                        end: Box::new(en),
                        inclusive: *inclusive,
                    },
                    ty,
                    res: Resolution::None,
                    span,
                };
            }
            // `e is T` -> a Bool test.
            ExprKind::Is { expression, type_ann } => {
                let v = self.lower_expr(expression);
                let bool_ty = BackendTy::Bool;
                // `v is SomeClass` — a real class membership test.
                if let varn_core::TypeKind::Named(n, _) = &type_ann.kind {
                    if let Some(class) = self.m.names.class_id(n) {
                        return TirExpr {
                            kind: TirExprKind::TypeTest { value: Box::new(v), class },
                            ty: bool_ty,
                            res: Resolution::None,
                            span,
                        };
                    }
                }
                // `v is int` / `is decimal` / `is str` … — a runtime type-name
                // check. The tag names match the keyword exactly.
                let tag_name: Option<&'static str> = match &type_ann.kind {
                    varn_core::TypeKind::Intrinsic(t) => Some(t.name()),
                    varn_core::TypeKind::Named(n, _) => {
                        varn_core::TypeTag::from_str(n).map(|t| t.name())
                    }
                    _ => None,
                };
                if let Some(name) = tag_name {
                    let got = TirExpr {
                        kind: TirExprKind::Unary {
                            op: TirUnOp::Typeof,
                            operand: Box::new(v),
                        },
                        ty: BackendTy::Str,
                        res: Resolution::None,
                        span,
                    };
                    let want = TirExpr {
                        kind: TirExprKind::StrLit(Rc::from(name)),
                        ty: BackendTy::Str,
                        res: Resolution::None,
                        span,
                    };
                    return TirExpr {
                        kind: TirExprKind::Binary {
                            op: TirBinOp::Eq,
                            lhs: Box::new(got),
                            rhs: Box::new(want),
                        },
                        ty: bool_ty,
                        res: Resolution::None,
                        span,
                    };
                }
                // An unresolved type: treat the value's truthiness as the test
                // (matches the prior behaviour for the shapes we cannot check).
                return self.cast_to(v, bool_ty);
            }
            // `import.meta.x` / `new.target` — a by-name field read.
            ExprKind::MetaAccess { target, property } => {
                let obj = self.lower_expr(target);
                return TirExpr {
                    kind: TirExprKind::Field { object: Box::new(obj), name: property.clone() },
                    ty,
                    res: Resolution::ByName { name: property.clone(), why: DynReason::Unannotated },
                    span,
                };
            }
            // `super` on its own — the parent instance; typed as this class.
            ExprKind::Super => {
                let sty = self
                    .this_class
                    .map(BackendTy::Class)
                    .unwrap_or(BackendTy::Dynamic(DynReason::Unannotated));
                return TirExpr { kind: TirExprKind::Var, ty: sty, res: Resolution::None, span };
            }
            ExprKind::TaggedTemplate { tag, template } => {
                use varn_core::ast::TemplatePart;
                let ExprKind::Template { parts } = &template.kind else {
                    return self.lower_expr(template);
                };
                // `tag`strings ${a} more ${b}`` -> `tag([s0, s1, s2], a, b)`,
                // where `strings` has one more entry than the interpolations
                // (an empty string fills any gap).
                let str_lit = |s: &str| TirExpr {
                    kind: TirExprKind::StrLit(Rc::from(s)),
                    ty: BackendTy::Str,
                    res: Resolution::None,
                    span,
                };
                let mut strings: Vec<TirArrayEl> = Vec::new();
                let mut values: Vec<TirArg> = Vec::new();
                let mut cur = String::new();
                for part in parts {
                    match part {
                        TemplatePart::Literal(s) => cur.push_str(s),
                        TemplatePart::Interpolation(e) => {
                            strings.push(TirArrayEl::Expr(str_lit(&cur)));
                            cur.clear();
                            let v = self.lower_expr(e);
                            values.push(TirArg::Expr(v));
                        }
                    }
                }
                strings.push(TirArrayEl::Expr(str_lit(&cur)));

                let strings_arr = TirExpr {
                    kind: TirExprKind::ArrayLit(strings),
                    ty: BackendTy::Array(self.tt.intern(BackendTy::Str)),
                    res: Resolution::None,
                    span,
                };
                let mut all_args = vec![TirArg::Expr(strings_arr)];
                all_args.extend(values);

                // `obj.tag`…`` keeps `obj` as the receiver.
                if let ExprKind::Member { object, property, computed: false, .. } = &tag.kind {
                    if let Some(name) = Self::member_name(property) {
                        let recv = self.lower_expr(object);
                        return TirExpr {
                            kind: TirExprKind::MethodCall {
                                recv: Box::new(recv),
                                name,
                                args: all_args,
                            },
                            ty,
                            res: Resolution::None,
                            span,
                        };
                    }
                }
                let callee = self.lower_expr(tag);
                let res = match &tag.kind {
                    ExprKind::Identifier { name } => Resolution::ByName {
                        name: name.clone(),
                        why: DynReason::Unannotated,
                    },
                    _ => Resolution::None,
                };
                return TirExpr {
                    kind: TirExprKind::Call { callee: Box::new(callee), args: all_args },
                    ty,
                    res,
                    span,
                };
            }

            ExprKind::Conditional { test, consequent, alternate } => {
                let cond = self.lower_expr(test);
                let cond = self.cast_to(cond, BackendTy::Bool);
                let then_val = self.lower_expr(consequent);
                let else_val = self.lower_expr(alternate);
                let (then_val, else_val) = if then_val.ty == else_val.ty
                    || matches!(ty, BackendTy::Dynamic(_))
                {
                    (then_val, else_val)
                } else {
                    (self.cast_to(then_val, ty), self.cast_to(else_val, ty))
                };
                return TirExpr {
                    kind: TirExprKind::Select {
                        cond: Box::new(cond),
                        then_val: Box::new(then_val),
                        else_val: Box::new(else_val),
                    },
                    ty,
                    res: Resolution::None,
                    span,
                };
            }

            // `Missing` (a parse hole) and a class expression have no runtime
            // value we model: a well-formed null.
            _ => None,
        };

        match kind {
            Some(kind) => TirExpr { kind, ty, res: Resolution::None, span },
            None => TirExpr {
                kind: TirExprKind::NullLit,
                ty: BackendTy::Dynamic(DynReason::Unannotated),
                res: Resolution::None,
                span,
            },
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
        let lhs = self.lower_expr(left);
        let rhs = self.lower_expr(right);

        let Some(top) = bin_op(op) else {
            // `instanceof` -> a class test where possible, else a Bool cast;
            // `in` -> a Bool membership test.
            if op == BinaryOp::Instanceof {
                if let ExprKind::Identifier { name } = &right.kind {
                    if let Some(class) = self.m.names.class_id(name) {
                        return TirExpr {
                            kind: TirExprKind::TypeTest { value: Box::new(lhs), class },
                            ty: BackendTy::Bool,
                            res: Resolution::None,
                            span,
                        };
                    }
                    // An intrinsic (`Array`, `Error`) or imported class — a
                    // real `instanceof` against the named global.
                    return TirExpr {
                        kind: TirExprKind::Binary {
                            op: TirBinOp::Instanceof,
                            lhs: Box::new(lhs),
                            rhs: Box::new(rhs),
                        },
                        ty: BackendTy::Bool,
                        res: Resolution::None,
                        span,
                    };
                }
            }
            if op == BinaryOp::In {
                return TirExpr {
                    kind: TirExprKind::Binary {
                        op: TirBinOp::In,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    },
                    ty: BackendTy::Bool,
                    res: Resolution::None,
                    span,
                };
            }
            return self.cast_to(lhs, BackendTy::Bool);
        };

        let (lhs, rhs, node_ty) = self.coerce_binary_operands(top, lhs, rhs, ty);
        TirExpr {
            kind: TirExprKind::Binary { op: top, lhs: Box::new(lhs), rhs: Box::new(rhs) },
            ty: node_ty,
            res: Resolution::None,
            span,
        }
    }

    /// Make a `Binary`'s operands satisfy the verifier: if either is dynamic
    /// the node is generic; otherwise both are cast to a common scalar and the
    /// result type follows `varn_core::numeric`.
    fn coerce_binary_operands(
        &self,
        op: TirBinOp,
        lhs: TirExpr,
        rhs: TirExpr,
        checked: BackendTy,
    ) -> (TirExpr, TirExpr, BackendTy) {
        let is_cmp = matches!(
            op,
            TirBinOp::Eq | TirBinOp::Ne | TirBinOp::Lt | TirBinOp::Le | TirBinOp::Gt | TirBinOp::Ge
        );
        let l = lhs.ty.non_nullable(self.tt);
        let r = rhs.ty.non_nullable(self.tt);

        if matches!(l, BackendTy::Dynamic(_)) || matches!(r, BackendTy::Dynamic(_)) {
            // Comparisons still have to say Bool; a generic arithmetic node is
            // dynamic. Either way one dynamic operand is enough — no cast.
            let ty = if is_cmp { BackendTy::Bool } else { BackendTy::Dynamic(DynReason::Unannotated) };
            return (lhs, rhs, ty);
        }
        if l == r {
            let ty = if is_cmp {
                BackendTy::Bool
            } else if op == TirBinOp::Div && l == BackendTy::Int {
                BackendTy::Float
            } else {
                l
            };
            return (lhs, rhs, ty);
        }
        // Mixed scalars: cast both to the wider one (float wins over int),
        // else to the left operand's type.
        let common = if l == BackendTy::Float || r == BackendTy::Float {
            BackendTy::Float
        } else if matches!(l, BackendTy::Str) || matches!(r, BackendTy::Str) {
            BackendTy::Str
        } else {
            l
        };
        let lhs = self.cast_to(lhs, common);
        let rhs = self.cast_to(rhs, common);
        let ty = if is_cmp { BackendTy::Bool } else { common };
        let _ = checked;
        (lhs, rhs, ty)
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
        // `&&` / `||` -> a `Select`: the operator mentions the left operand
        // once (as the condition) and the right once (as one arm), so a branch
        // preserves both the short-circuit and any side effect exactly, with
        // no need for a hoisted temp. `is_pure` is not consulted for these.
        let (cond, then_val, else_val) = match op {
            LogicalOp::And | LogicalOp::Or => {
                let l = self.lower_expr(left);
                let l = self.cast_to(l, BackendTy::Bool);
                let r = self.lower_expr(right);
                let r = self.cast_to(r, BackendTy::Bool);
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
        // A Select's arms must agree (or the result be a union). Cast both to
        // the result type when they diverge.
        let (then_val, else_val) = if then_val.ty == else_val.ty
            || matches!(ty, BackendTy::Dynamic(_))
        {
            (then_val, else_val)
        } else {
            (self.cast_to(then_val, ty), self.cast_to(else_val, ty))
        };
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
            let name = Self::member_name(property).unwrap_or_else(|| Rc::from("<member>"));
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
            let null_arm = TirExpr {
                kind: TirExprKind::NullLit,
                ty: access.ty,
                res: Resolution::None,
                span,
            };
            let result_ty = access.ty;
            return TirExpr {
                kind: TirExprKind::Select {
                    cond: Box::new(is_null),
                    then_val: Box::new(null_arm),
                    else_val: Box::new(access),
                },
                ty: result_ty,
                res: Resolution::None,
                span,
            };
        }

        let obj = self.lower_expr(object);

        if computed {
            // `obj[a..b]` — a slice. Lower to `obj.slice(a, b')` where an
            // inclusive range bumps the end by one.
            if let ExprKind::Range { start, end, inclusive } = &property.kind {
                let s = self.lower_expr(start);
                let mut e = self.lower_expr(end);
                if *inclusive {
                    e = TirExpr {
                        kind: TirExprKind::Binary {
                            op: TirBinOp::Add,
                            lhs: Box::new(e),
                            rhs: Box::new(int_lit(1)),
                        },
                        ty: BackendTy::Int,
                        res: Resolution::None,
                        span,
                    };
                }
                return TirExpr {
                    kind: TirExprKind::MethodCall {
                        recv: Box::new(obj),
                        name: Rc::from("slice"),
                        args: vec![TirArg::Expr(s), TirArg::Expr(e)],
                    },
                    ty,
                    res: Resolution::ByName {
                        name: Rc::from("slice"),
                        why: DynReason::Unannotated,
                    },
                    span,
                };
            }
            // `obj[key]`. An array index is pinned to the element type; other
            // receivers are unconstrained by the verifier.
            let index = self.lower_expr(property);
            let node_ty = match obj.ty.non_nullable(self.tt) {
                BackendTy::Array(el) => self.tt.get(el),
                _ => ty,
            };
            return TirExpr {
                kind: TirExprKind::Index { object: Box::new(obj), index: Box::new(index) },
                ty: node_ty,
                res: Resolution::None,
                span,
            };
        }

        let name = Self::member_name(property).unwrap_or_else(|| Rc::from("<member>"));

        // `recv.p` the checker resolved to an extension getter: `__extget(recv)`
        // — an extension call so `recv` lands in the `this` slot.
        if let Some(mangled) = self.m.ext_members.get(&property.range.start.offset).cloned() {
            return TirExpr {
                kind: TirExprKind::ExtensionCall {
                    func: mangled,
                    recv: Box::new(obj),
                    args: vec![],
                },
                ty,
                res: Resolution::None,
                span,
            };
        }

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
            return_ty: BackendTy::Dynamic(DynReason::Unannotated),
            locals: vec![],
            body: vec![],
            has_this: false,
            this_class: None,
            is_async,
            is_generator,
            has_rest: params.last().is_some_and(|p| p.is_rest),
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
        let captures = std::mem::take(&mut sub.captures);

        self.out_closures[slot] = TirFunction {
            name: Rc::from("<closure>"),
            sig: SigId(0),
            params: param_tys,
            return_ty: BackendTy::Dynamic(DynReason::Unannotated),
            locals,
            body: stmts,
            has_this: false,
            this_class: None,
            is_async,
            is_generator,
            has_rest: params.last().is_some_and(|p| p.is_rest),
        };

        // Resolve each captured name against THIS (the enclosing) frame. A
        // name that is itself an upvalue here chains through as `ParentUpvalue`
        // — `resolve_name` records it in `self.captures` on the way.
        let upvalues: Vec<varn_tir::TirUpvalue> = captures
            .iter()
            .map(|name| match self.resolve_name(name) {
                Resolution::Local(id) => varn_tir::TirUpvalue::ParentLocal(id.0),
                Resolution::Param(i) => varn_tir::TirUpvalue::ParentParam(i),
                Resolution::Upvalue(i) => varn_tir::TirUpvalue::ParentUpvalue(i),
                // A capture that resolves to a global here is not really a
                // capture; the closure body will read it as a global too. Use
                // a param-0 placeholder that the backend simply never reads.
                _ => varn_tir::TirUpvalue::ParentUpvalue(0),
            })
            .collect();

        TirExpr {
            kind: TirExprKind::Closure { func: func_id, upvalues },
            ty,
            res: Resolution::None,
            span,
        }
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

    fn lower_new(&mut self, call_id: AstId, callee: &Expr, args: &[Arg], ty: BackendTy, span: Span) -> TirExpr {
        let class = match &callee.kind {
            ExprKind::Identifier { name } => self.m.names.class_id(name),
            // `new NS.Class(…)` — a namespaced class is still a module global;
            // the qualifier only scopes the name.
            ExprKind::Member { property, computed: false, .. } => {
                Self::member_name(property).and_then(|n| self.m.names.class_id(&n))
            }
            _ => None,
        };
        let targs = self.lower_call_args(call_id, args);
        match class {
            Some(class) => TirExpr {
                kind: TirExprKind::New { class, args: targs },
                ty,
                res: Resolution::None,
                span,
            },
            // An imported or dynamic constructor: a by-name call on the callee.
            None => {
                let c = self.lower_expr(callee);
                TirExpr {
                    kind: TirExprKind::Call { callee: Box::new(c), args: targs },
                    ty,
                    res: Resolution::ByName {
                        name: Rc::from("<new>"),
                        why: DynReason::Unannotated,
                    },
                    span,
                }
            }
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

    /// Argument list for a call, laid out positionally. When the checker
    /// recorded a named-argument mapping for this call, arguments are
    /// reordered to parameter position and omitted slots become a bare `null`
    /// — the callee's own default-guard prologue fills them in.
    fn lower_call_args(&mut self, call_id: AstId, args: &[Arg]) -> Vec<TirArg> {
        match self.m.call_mappings.get(&call_id).cloned() {
            Some(mapping) => mapping
                .iter()
                .map(|opt| match opt {
                    Some(i) => match &args[*i] {
                        Arg::Positional(e) | Arg::Named { value: e, .. } => {
                            TirArg::Expr(self.lower_expr(e))
                        }
                        Arg::Spread(e) => TirArg::Spread(self.lower_expr(e)),
                    },
                    None => TirArg::Expr(TirExpr {
                        kind: TirExprKind::NullLit,
                        ty: BackendTy::Dynamic(DynReason::Unannotated),
                        res: Resolution::None,
                        span: Span::EMPTY,
                    }),
                })
                .collect(),
            None => args.iter().map(|a| self.lower_arg(a)).collect(),
        }
    }

    fn lower_call(
        &mut self,
        call_id: AstId,
        callee: &Expr,
        args: &[Arg],
        ty: BackendTy,
        span: Span,
    ) -> TirExpr {
        // `super(args)` — base constructor.
        if matches!(callee.kind, ExprKind::Super) {
            let targs = self.lower_call_args(call_id, args);
            return TirExpr {
                kind: TirExprKind::SuperCall { args: targs },
                ty,
                res: Resolution::None,
                span,
            };
        }
        // `super.name(args)` — base method, bypassing the vtable.
        if let ExprKind::Member { object, property, computed: false, .. } = &callee.kind {
            if matches!(object.kind, ExprKind::Super) {
                if let Some(name) = Self::member_name(property) {
                    let targs = self.lower_call_args(call_id, args);
                    return TirExpr {
                        kind: TirExprKind::SuperMethodCall { name, args: targs },
                        ty,
                        res: Resolution::None,
                        span,
                    };
                }
            }
        }

        // Free call on an identifier: `f(args)`.
        if let ExprKind::Identifier { name } = &callee.kind {
            let c = self.lower_expr(callee);
            let targs = self.lower_call_args(call_id, args);
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

        // Method call: `recv.name(args)`. A computed or non-nameable callee
        // (`obj[k]()`, `(f())()`) is a by-name call on the lowered callee.
        let (object, property) = match &callee.kind {
            ExprKind::Member { object, property, computed: false, .. } => (object, property),
            _ => return self.by_name_call(call_id, callee, args, ty, span),
        };
        let Some(name) = Self::member_name(property) else {
            return self.by_name_call(call_id, callee, args, ty, span);
        };

        // `recv.m(args)` the checker resolved to an extension function.
        if let Some(mangled) = self.m.ext_calls.get(&span.start).cloned() {
            let recv = self.lower_expr(object);
            let targs = self.lower_call_args(call_id, args);
            return TirExpr {
                kind: TirExprKind::ExtensionCall {
                    func: mangled,
                    recv: Box::new(recv),
                    args: targs,
                },
                ty,
                res: Resolution::None,
                span,
            };
        }

        // `E.V(args)` — an enum variant with a payload.
        if let Some((enum_id, tag)) = self.enum_variant(object, &name) {
            let vargs = self.lower_call_args(call_id, args);
            return TirExpr {
                kind: TirExprKind::MakeVariant { args: vargs },
                ty: BackendTy::Enum(enum_id),
                res: Resolution::EnumVariant { enum_id, tag },
                span,
            };
        }

        let recv = self.lower_expr(object);
        let targs = self.lower_call_args(call_id, args);

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

    /// A call whose callee has no static resolution: `Call` + `ByName`.
    fn by_name_call(&mut self, call_id: AstId, callee: &Expr, args: &[Arg], ty: BackendTy, span: Span) -> TirExpr {
        let c = self.lower_expr(callee);
        let targs = self.lower_call_args(call_id, args);
        TirExpr {
            kind: TirExprKind::Call { callee: Box::new(c), args: targs },
            ty,
            res: Resolution::ByName { name: Rc::from("<call>"), why: DynReason::Unannotated },
            span,
        }
    }

    fn lower_unary(&mut self, op: UnaryOp, operand: &Expr, ty: BackendTy, span: Span) -> TirExpr {
        let top = match op {
            UnaryOp::Minus => TirUnOp::Neg,
            UnaryOp::Not => TirUnOp::Not,
            UnaryOp::BitNot => TirUnOp::BitNot,
            UnaryOp::Plus => return self.lower_expr(operand), // unary + is identity
            // `typeof x` yields the runtime type name as a string.
            UnaryOp::Typeof => {
                let inner = self.lower_expr(operand);
                return TirExpr {
                    kind: TirExprKind::Unary { op: TirUnOp::Typeof, operand: Box::new(inner) },
                    ty: BackendTy::Str,
                    res: Resolution::None,
                    span,
                };
            }
        };
        // Only `IsNull` is type-checked by the verifier (must be Bool); the
        // arithmetic/logical unaries are not, so the checker's type stands.
        let inner = self.lower_expr(operand);
        TirExpr {
            kind: TirExprKind::Unary { op: top, operand: Box::new(inner) },
            ty,
            res: Resolution::None,
            span,
        }
    }
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
        A::BitAndAssign => TirBinOp::BitAnd,
        A::BitOrAssign => TirBinOp::BitOr,
        A::BitXorAssign => TirBinOp::BitXor,
        A::ShlAssign => TirBinOp::Shl,
        A::ShrAssign => TirBinOp::Shr,
        A::UShrAssign => TirBinOp::Ushr,
        // `&&=` / `||=` / `??=` short-circuit — lowered by the caller.
        A::AndAssign | A::OrAssign | A::NullishAssign => return Err(()),
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

