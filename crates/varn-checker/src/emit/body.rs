use crate::checker::TypeEntry;
use crate::emit::tables::NameIndex;
use crate::emit::ty::{lower_type, NameResolver};
use rustc_hash::{FxHashMap, FxHashSet};
use std::rc::Rc;
use varn_core::ast::operators::{BinaryOp, LogicalOp, UnaryOp};
use varn_core::{Atom, AtomInterner};
use varn_core::ast::pattern::{MatchBinding, MatchPattern};
use varn_core::ast::{
    Arg, ArrayEl, AstArena, AstId, ExprId, ExprKind, MatchBody, MatchCase, ObjectProp, Pattern,
    PropKey, StmtId, StmtKind,
};
use varn_tir::{
    BackendTy, ClassId, ClassInfo, DynReason, EnumId, EnumInfo, LocalId, Resolution, SigId,
    Signature, Span, TirArg, TirArrayEl, TirBinOp, TirExpr, TirExprKind, TirFunction,
    TirObjectEntry, TirStmt, TirUnOp, TyTable,
};

#[derive(Clone, Copy)]
pub(super) struct ModuleCtx<'a> {
    pub names: &'a NameIndex,
    pub classes: &'a [ClassInfo],
    pub enums: &'a [EnumInfo],

    pub globals: &'a FxHashMap<Rc<str>, u32>,

    pub fns: &'a FxHashMap<Atom, (u32, u32)>,

    pub call_mappings: &'a FxHashMap<AstId, Vec<Option<usize>>>,

    pub ext_calls: &'a FxHashMap<u32, Rc<str>>,

    pub ext_members: &'a FxHashMap<u32, Rc<str>>,

    pub ext_set_members: &'a FxHashMap<u32, Rc<str>>,

    pub core_ops: &'a FxHashSet<(Rc<str>, Rc<str>)>,

    pub math_intrinsics: &'a FxHashMap<Atom, u8>,

    pub interner: &'a AtomInterner,

    pub checker_table: &'a crate::types::CheckerTyTable,
}

pub(super) struct FnEmitter<'a> {
    pub ast_arena: &'a AstArena,
    pub expr_table: &'a FxHashMap<AstId, TypeEntry>,
    pub tt: &'a mut TyTable,
    m: ModuleCtx<'a>,
    pub signatures: &'a mut Vec<Signature>,

    out_closures: &'a mut Vec<TirFunction>,
    closure_base: u32,
    pub locals: Vec<BackendTy>,
    scopes: Vec<FxHashMap<Rc<str>, LocalId>>,
    params: Vec<Rc<str>>,
    this_class: Option<ClassId>,

    this_enum: Option<EnumId>,

    top_level: bool,

    outer_names: FxHashSet<Rc<str>>,

    captures: Vec<Rc<str>>,

    pending: Vec<TirStmt>,

    disposables: Vec<Vec<TirExpr>>,
}

enum ClosureBody {
    Expr(ExprId),
    Stmt(StmtId),
}

fn splice_finally_before_exits(stmts: Vec<TirStmt>, fin: &[TirStmt]) -> Vec<TirStmt> {
    splice_finally_impl(stmts, fin, false)
}

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
            TirStmt::If {
                cond,
                then_body,
                else_body,
            } => out.push(TirStmt::If {
                cond,
                then_body: splice_finally_impl(then_body, fin, on_throw),
                else_body: splice_finally_impl(else_body, fin, on_throw),
            }),
            TirStmt::Loop { cond, body } => {
                out.push(TirStmt::Loop {
                    cond,
                    body: splice_returns_only(body, fin),
                });
            }
            TirStmt::Try {
                body,
                catch_local,
                catch_body,
            } => out.push(TirStmt::Try {
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
            TirStmt::If {
                cond,
                then_body,
                else_body,
            } => out.push(TirStmt::If {
                cond,
                then_body: splice_returns_only(then_body, fin),
                else_body: splice_returns_only(else_body, fin),
            }),
            TirStmt::Loop { cond, body } => out.push(TirStmt::Loop {
                cond,
                body: splice_returns_only(body, fin),
            }),
            TirStmt::Try {
                body,
                catch_local,
                catch_body,
            } => out.push(TirStmt::Try {
                body: splice_returns_only(body, fin),
                catch_local,
                catch_body: splice_returns_only(catch_body, fin),
            }),
            other => out.push(other),
        }
    }
    out
}

fn pattern_lead(p: &Pattern, interner: &AtomInterner) -> Rc<str> {
    match p {
        Pattern::Identifier { name, .. } => Rc::from(interner.resolve(*name)),
        _ => Rc::from("_"),
    }
}

#[derive(Clone, Copy)]
enum MatchDest {
    Statement,
    Return,
    Assign(LocalId),
}

fn span_of(ast_arena: &AstArena, e: ExprId) -> Span {
    let range = ast_arena.expr(e).range;
    Span {
        start: range.start.offset,
        end: range.end.offset,
    }
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
        ast_arena: &'a AstArena,
        expr_table: &'a FxHashMap<AstId, TypeEntry>,
        tt: &'a mut TyTable,
        m: ModuleCtx<'a>,
        signatures: &'a mut Vec<Signature>,
        out_closures: &'a mut Vec<TirFunction>,
        closure_base: u32,
        params: Vec<Rc<str>>,
    ) -> Self {
        FnEmitter {
            ast_arena,
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

    fn fresh_local(&mut self, ty: BackendTy) -> LocalId {
        let id = LocalId(self.locals.len() as u32);
        self.locals.push(ty);
        id
    }

    fn hoist(&mut self, e: TirExpr) -> TirExpr {
        let ty = e.ty;
        let span = e.span;
        let local = self.fresh_local(ty);
        self.pending.push(TirStmt::Let {
            local,
            ty,
            init: Some(e),
        });
        TirExpr {
            kind: TirExprKind::Var,
            ty,
            res: Resolution::Local(local),
            span,
        }
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

    pub fn lower_outer_expr(&mut self, e: ExprId) -> (Vec<TirStmt>, TirExpr) {
        let x = self.lower_expr(e);
        (std::mem::take(&mut self.pending), x)
    }

    pub fn lower_expression(&mut self, e: ExprId) -> TirExpr {
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

    /// The op-id-addressable core class a receiver type belongs to, if any.
    /// Never returns a user class — only the native reference types whose
    /// runtime tag is guaranteed to match the static type. The numeric
    /// primitives are deliberately excluded: implicit coercion (`int` → `bigint`
    /// / `decimal` / `float`) leaves the value int-tagged while the static type
    /// says otherwise, so an op-id keyed on the static type would misdispatch.
    fn core_class_name(&self, ty: BackendTy) -> Option<&'static str> {
        use varn_core::TypeTag as T;
        let tag = match ty.non_nullable(self.tt) {
            BackendTy::Array(_) => T::Array,
            BackendTy::Str => T::Str,
            BackendTy::Map(..) => T::Map,
            BackendTy::Set(_) => T::Set,
            _ => return None,
        };
        varn_core::op_id::core_class_name(tag)
    }

    fn expr_ty(&mut self, e: ExprId) -> BackendTy {
        let names = self.m.names;
        let table = self.m.checker_table;
        let interner = self.m.interner;
        match self.expr_table.get(&e.index()) {
            Some(entry) => lower_type(&entry.ty, table, interner, self.tt, names),
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
        // A prelude / host symbol at its fixed native-layout index.
        if let Some(idx) = varn_builtins::native_global_index(name) {
            return Resolution::NativeGlobal(idx);
        }
        Resolution::ByName {
            name: Rc::from(name),
            why: DynReason::Unannotated,
        }
    }

    fn bind_local(&mut self, name: Rc<str>, ty: BackendTy) -> LocalId {
        let id = LocalId(self.locals.len() as u32);
        self.locals.push(ty);
        self.scopes.last_mut().unwrap().insert(name, id);
        id
    }

    pub fn lower_block(&mut self, stmts: &[StmtId]) -> Vec<TirStmt> {
        self.scopes.push(FxHashMap::default());
        self.disposables.push(Vec::new());
        let mut out = Vec::new();
        for &s in stmts {
            out.extend(self.lower_stmt(s));
        }

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

    pub fn lower_stmt_as_block(&mut self, s: StmtId) -> Vec<TirStmt> {
        match &self.ast_arena.stmt(s).kind {
            StmtKind::Block { stmts } => {
                let stmts = stmts.clone();
                self.lower_block(&stmts)
            }
            _ => self.lower_stmt(s),
        }
    }

    fn lower_stmt(&mut self, s: StmtId) -> Vec<TirStmt> {
        let one = |s: TirStmt| vec![s];
        let drained = |em: &mut Self, built: Vec<TirStmt>| {
            let mut out = std::mem::take(&mut em.pending);
            out.extend(built);
            out
        };
        match &self.ast_arena.stmt(s).kind {
            StmtKind::Block { stmts } => self.lower_block(stmts),
            StmtKind::Expr { expression } => {
                let expression = *expression;
                if let ExprKind::Match { subject, cases } = &self.ast_arena.expr(expression).kind {
                    let (subject, cases) = (*subject, cases);
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
                let argument = *argument;
                if let Some(arg) = argument {
                    if let ExprKind::Match { subject, cases } = &self.ast_arena.expr(arg).kind {
                        let (subject, cases) = (*subject, cases);
                        return self.lower_match(subject, cases, MatchDest::Return);
                    }
                }
                let a = argument.map(|a| self.lower_expr(a));
                drained(self, one(TirStmt::Return(a)))
            }
            StmtKind::Throw { argument } => {
                let a = self.lower_expr(*argument);
                drained(self, one(TirStmt::Throw(a)))
            }
            StmtKind::Break { .. } => one(TirStmt::Break),
            StmtKind::Continue { .. } => one(TirStmt::Continue),

            StmtKind::If {
                test,
                consequent,
                alternate,
            } => {
                let (test, consequent, alternate) = (*test, *consequent, *alternate);
                let cond = self.lower_cond(test);
                let mut out = std::mem::take(&mut self.pending);
                let then_body = self.lower_stmt_as_block(consequent);
                let else_body = alternate
                    .map(|a| self.lower_stmt_as_block(a))
                    .unwrap_or_default();
                out.push(TirStmt::If {
                    cond,
                    then_body,
                    else_body,
                });
                out
            }

            StmtKind::While { test, body } => {
                let (test, body) = (*test, *body);
                let cond = self.lower_cond(test);
                let cond_pending = std::mem::take(&mut self.pending);
                let body = self.lower_stmt_as_block(body);
                if cond_pending.is_empty() {
                    one(TirStmt::Loop { cond, body })
                } else {
                    let mut loop_body = cond_pending;
                    loop_body.push(TirStmt::If {
                        cond,
                        then_body: vec![],
                        else_body: vec![TirStmt::Break],
                    });
                    loop_body.extend(body);
                    one(TirStmt::Loop {
                        cond: bool_lit(true),
                        body: loop_body,
                    })
                }
            }

            StmtKind::For {
                init,
                test,
                update,
                body,
            } => {
                let (test, update, body) = (*test, *update, *body);
                self.lower_for(init.as_deref(), test, update, body)
            }

            StmtKind::DoWhile { body, test } => {
                let (body, test) = (*body, *test);
                let mut loop_body = self.lower_stmt_as_block(body);
                let cond = self.lower_cond(test);
                loop_body.extend(std::mem::take(&mut self.pending));
                loop_body.push(TirStmt::If {
                    cond,
                    then_body: vec![],
                    else_body: vec![TirStmt::Break],
                });
                one(TirStmt::Loop {
                    cond: bool_lit(true),
                    body: loop_body,
                })
            }

            StmtKind::ForOf {
                left,
                right,
                body,
                is_await,
                ..
            } => self.lower_for_of(left, *right, *body, *is_await),
            StmtKind::ForIn {
                left, right, body, ..
            } => self.lower_for_in(left, *right, *body),

            StmtKind::Try {
                block,
                catches,
                finally,
            } => self.lower_try(*block, catches, *finally),

            StmtKind::Switch {
                discriminant,
                cases,
            } => self.lower_switch(*discriminant, cases),

            StmtKind::Using { declarations, .. } => {
                let dyn_ty = BackendTy::Dynamic(DynReason::Unannotated);
                let mut out = Vec::new();
                for d in declarations {
                    let init = d.init.map(|e| self.lower_expr(e));
                    match &d.id {
                        Pattern::Identifier { name, .. } => {
                            let ty = init.as_ref().map(|e| e.ty).unwrap_or(dyn_ty);
                            let local = self.bind_local(Rc::from(self.m.interner.resolve(*name)), ty);
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

                        pat => {
                            let src = init.unwrap_or_else(|| placeholder(DynReason::Unannotated));
                            out.extend(std::mem::take(&mut self.pending));
                            let src = self.hoist(src);
                            out.extend(std::mem::take(&mut self.pending));
                            if let Some(frame) = self.disposables.last_mut() {
                                frame.push(src.clone());
                            }
                            self.bind_pattern(pat, src, &mut out);
                        }
                    }
                }
                out
            }

            StmtKind::Labeled { body, .. } => self.lower_stmt_as_block(*body),
        }
    }

    fn catch_type_names(&self, t: &varn_core::ast::TypeNode) -> Vec<Rc<str>> {
        use varn_core::TypeKind;
        match &t.kind {
            TypeKind::Named(n, _) => vec![Rc::from(self.m.interner.resolve(*n))],
            TypeKind::Union(items) | TypeKind::Intersection(items) => items
                .iter()
                .flat_map(|x| self.catch_type_names(x))
                .collect(),
            _ => vec![],
        }
    }

    fn instance_of_name(&self, value: TirExpr, name: &str) -> TirExpr {
        let span = value.span;
        if let Some(class) = self.m.names.class_id(name) {
            return TirExpr {
                kind: TirExprKind::TypeTest {
                    value: Box::new(value),
                    class,
                },
                ty: BackendTy::Bool,
                res: Resolution::None,
                span,
            };
        }
        let rhs = TirExpr {
            kind: TirExprKind::Var,
            ty: BackendTy::Dynamic(DynReason::Unannotated),
            res: Resolution::ByName {
                name: Rc::from(name),
                why: DynReason::Unannotated,
            },
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
        block: StmtId,
        catches: &[varn_core::ast::CatchClause],
        finally: Option<StmtId>,
    ) -> Vec<TirStmt> {
        let body = self.lower_stmt_as_block(block);
        let dyn_ty = BackendTy::Dynamic(DynReason::Unannotated);

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
                if this.m.interner.resolve(*name) != "<catch>" {
                    let alias = this.bind_local(Rc::from(this.m.interner.resolve(*name)), dyn_ty);
                    out.push(TirStmt::Let {
                        local: alias,
                        ty: dyn_ty,
                        init: Some(e_var(Span::EMPTY)),
                    });
                }
            }
            out.extend(this.lower_stmt_as_block(c.body));
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
            chain = vec![TirStmt::If {
                cond,
                then_body,
                else_body: std::mem::take(&mut chain),
            }];
        }
        self.scopes.pop();
        let catch_body = chain;

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
        let mut out = vec![TirStmt::Try {
            body,
            catch_local,
            catch_body,
        }];
        out.extend(fin);
        out
    }

    fn lower_switch(
        &mut self,
        discriminant: ExprId,
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
        let Some(case) = cases.get(i) else {
            return vec![];
        };
        self.scopes.push(FxHashMap::default());
        let body: Vec<TirStmt> = case.body.iter().flat_map(|&s| self.lower_stmt(s)).collect();
        self.scopes.pop();
        let rest = self.switch_cases(d, cases, i + 1);
        match case.test {
            None => {
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
                pre.push(TirStmt::If {
                    cond,
                    then_body: body,
                    else_body: rest,
                });
                pre
            }
        }
    }

    fn lower_for(
        &mut self,
        init: Option<&varn_core::ast::ForInit>,
        test: Option<ExprId>,
        update: Option<ExprId>,
        body: StmtId,
    ) -> Vec<TirStmt> {
        use varn_core::ast::ForInit;
        let mut out = Vec::new();

        match init {
            Some(ForInit::Var { declarators, .. }) => {
                for d in declarators {
                    if let Pattern::Identifier { name, .. } = &d.id {
                        let iexpr = d.init.map(|e| self.lower_expr(e));
                        let ty = iexpr
                            .as_ref()
                            .map(|e| e.ty)
                            .unwrap_or(BackendTy::Dynamic(DynReason::Unannotated));
                        let local = self.bind_local(Rc::from(self.m.interner.resolve(*name)), ty);
                        out.extend(std::mem::take(&mut self.pending));
                        out.push(TirStmt::Let {
                            local,
                            ty,
                            init: iexpr,
                        });
                    }
                }
            }
            Some(ForInit::Expr(e)) => {
                let e = self.lower_expr(*e);
                out.extend(std::mem::take(&mut self.pending));
                out.push(TirStmt::Expr(e));
            }
            None => {}
        }

        if !has_continue(self.ast_arena, body) {
            let cond = test.map(|t| self.lower_cond(t)).unwrap_or_else(|| bool_lit(true));
            let cond_pending = std::mem::take(&mut self.pending);
            let has_cond_pending = !cond_pending.is_empty();
            let mut loop_body = if has_cond_pending {
                let mut p = cond_pending;
                p.push(TirStmt::If {
                    cond: cond.clone(),
                    then_body: vec![],
                    else_body: vec![TirStmt::Break],
                });
                p
            } else {
                Vec::new()
            };
            loop_body.extend(self.lower_stmt_as_block(body));
            if let Some(u) = update {
                let ue = self.lower_expr(u);
                loop_body.extend(std::mem::take(&mut self.pending));
                loop_body.push(TirStmt::Expr(ue));
            }
            let loop_cond = if has_cond_pending { bool_lit(true) } else { cond };
            out.push(TirStmt::Loop {
                cond: loop_cond,
                body: loop_body,
            });
            return out;
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
        out.push(TirStmt::Loop {
            cond: bool_lit(true),
            body: loop_body,
        });
        out
    }

    fn lower_for_in(&mut self, left: &Pattern, right: ExprId, body: StmtId) -> Vec<TirStmt> {
        let obj = self.lower_expr(right);
        let mut out = std::mem::take(&mut self.pending);
        let s = self.tt.intern(BackendTy::Str);
        let keys_ty = BackendTy::Array(s);
        let keys = TirExpr {
            kind: TirExprKind::ObjectKeys {
                operand: Box::new(obj),
            },
            ty: keys_ty,
            res: Resolution::None,
            span: Span::EMPTY,
        };
        let keys = self.hoist(keys);
        out.extend(std::mem::take(&mut self.pending));
        let Pattern::Identifier { name, .. } = left else {
            return self.lower_for_of_protocol(left, keys, out, body, false);
        };
        let name: Rc<str> = Rc::from(self.m.interner.resolve(*name));
        out.extend(self.for_of_over_array(&name, keys, BackendTy::Str, body));
        out
    }

    fn lower_for_of(
        &mut self,
        left: &Pattern,
        right: ExprId,
        body: StmtId,
        is_await: bool,
    ) -> Vec<TirStmt> {
        if !is_await {
            if let (
                ExprKind::Range {
                    start,
                    end,
                    inclusive,
                },
                Pattern::Identifier { name, .. },
            ) = (&self.ast_arena.expr(right).kind, left)
            {
                let (start, end, inclusive) = (*start, *end, *inclusive);
                let name: Rc<str> = Rc::from(self.m.interner.resolve(*name));
                let lo = self.lower_expr(start);
                let mut hi = self.lower_expr(end);
                if inclusive {
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

                let start = TirExpr {
                    kind: TirExprKind::Binary {
                        op: TirBinOp::Sub,
                        lhs: Box::new(lo),
                        rhs: Box::new(int_lit(1)),
                    },
                    ty: BackendTy::Int,
                    res: Resolution::None,
                    span: Span::EMPTY,
                };
                out.push(TirStmt::Let {
                    local: i,
                    ty: BackendTy::Int,
                    init: Some(start),
                });
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
                let step = TirStmt::Expr(TirExpr {
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
                });
                let mut loop_body = vec![
                    step,
                    TirStmt::If {
                        cond,
                        then_body: vec![],
                        else_body: vec![TirStmt::Break],
                    },
                ];
                loop_body.extend(self.lower_stmt_as_block(body));
                out.push(TirStmt::Loop {
                    cond: bool_lit(true),
                    body: loop_body,
                });
                return out;
            }
        }

        let iter = self.lower_expr(right);
        let mut out = std::mem::take(&mut self.pending);

        if is_await {
            return self.lower_for_of_protocol(left, iter, out, body, true);
        }

        let Pattern::Identifier { name, .. } = left else {
            return self.lower_for_of_protocol(left, iter, out, body, false);
        };

        let elem_ty = match iter.ty.non_nullable(self.tt) {
            BackendTy::Array(el) => self.tt.get(el),
            _ => return self.lower_for_of_protocol(left, iter, out, body, false),
        };
        let arr = self.hoist(iter);
        out.extend(std::mem::take(&mut self.pending));
        let name: Rc<str> = Rc::from(self.m.interner.resolve(*name));
        out.extend(self.for_of_over_array(&name, arr, elem_ty, body));
        out
    }

    fn for_of_over_array(
        &mut self,
        name: &Rc<str>,
        arr: TirExpr,
        elem_ty: BackendTy,
        body: StmtId,
    ) -> Vec<TirStmt> {
        let mut out = Vec::new();
        let idx = self.fresh_local(BackendTy::Int);

        out.push(TirStmt::Let {
            local: idx,
            ty: BackendTy::Int,
            init: Some(int_lit(-1)),
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

        let step = TirStmt::Expr(TirExpr {
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
        });
        let mut loop_body = vec![
            step,
            TirStmt::If {
                cond,
                then_body: vec![],
                else_body: vec![TirStmt::Break],
            },
            TirStmt::Let {
                local: x_local,
                ty: elem_ty,
                init: Some(elem),
            },
        ];
        loop_body.extend(self.lower_stmt_as_block(body));

        out.push(TirStmt::Loop {
            cond: bool_lit(true),
            body: loop_body,
        });
        out
    }

    fn lower_for_of_protocol(
        &mut self,
        left: &Pattern,
        src: TirExpr,
        mut out: Vec<TirStmt>,
        body: StmtId,
        is_await: bool,
    ) -> Vec<TirStmt> {
        let dyn_ty = BackendTy::Dynamic(DynReason::Unannotated);
        let by_name = |n: &str| Resolution::ByName {
            name: Rc::from(n),
            why: DynReason::Unannotated,
        };

        out.extend(std::mem::take(&mut self.pending));

        let iter_getter = TirExpr {
            kind: TirExprKind::IterInit {
                source: Box::new(src),
                is_async: is_await,
            },
            ty: dyn_ty,
            res: Resolution::None,
            span: Span::EMPTY,
        };
        let it = self.fresh_local(dyn_ty);
        out.push(TirStmt::Let {
            local: it,
            ty: dyn_ty,
            init: Some(iter_getter),
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
            kind: TirExprKind::Field {
                object: Box::new(recv),
                name: Rc::from(n),
            },
            ty: BackendTy::Dynamic(DynReason::Unannotated),
            res: Resolution::ByName {
                name: Rc::from(n),
                why: DynReason::Unannotated,
            },
            span: Span::EMPTY,
        };

        let mut next_call = TirExpr {
            kind: TirExprKind::MethodCall {
                recv: Box::new(it_var()),
                name: Rc::from("next"),
                args: vec![],
            },
            ty: dyn_ty,
            res: by_name("next"),
            span: Span::EMPTY,
        };
        if is_await {
            next_call = TirExpr {
                kind: TirExprKind::Await {
                    future: Box::new(next_call),
                },
                ty: dyn_ty,
                res: Resolution::None,
                span: Span::EMPTY,
            };
        }
        let mut loop_body = vec![
            TirStmt::Let {
                local: step,
                ty: dyn_ty,
                init: Some(next_call),
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

        out.push(TirStmt::Loop {
            cond: bool_lit(true),
            body: loop_body,
        });
        out
    }

    fn lower_match(
        &mut self,
        subject: ExprId,
        cases: &[MatchCase],
        dest: MatchDest,
    ) -> Vec<TirStmt> {
        let subj = self.lower_expr(subject);
        let mut out = std::mem::take(&mut self.pending);
        let s = self.hoist(subj);
        out.extend(std::mem::take(&mut self.pending));
        let chain = self.match_cases(&s, cases, 0, dest);
        out.extend(chain);
        out
    }

    fn lower_match_stmt(&mut self, subject: ExprId, cases: &[MatchCase]) -> Vec<TirStmt> {
        self.lower_match(subject, cases, MatchDest::Statement)
    }

    fn match_cases(
        &mut self,
        s: &TirExpr,
        cases: &[MatchCase],
        i: usize,
        dest: MatchDest,
    ) -> Vec<TirStmt> {
        let Some(case) = cases.get(i) else {
            return vec![];
        };

        self.scopes.push(FxHashMap::default());
        let (cond, bindings) = self.match_pattern(s, &case.pattern);

        let guard = case.guard.map(|g| {
            let gexpr = self.lower_expr(g);
            let pending = std::mem::take(&mut self.pending);
            (pending, gexpr)
        });
        let mut then_body: Vec<TirStmt> = Vec::new();

        let value = match &case.body {
            MatchBody::Expr(e) => self.lower_expr(*e),
            MatchBody::Block(stmt) => {
                then_body.extend(self.lower_stmt_as_block(*stmt));
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
                vec![TirStmt::If {
                    cond,
                    then_body: m,
                    else_body,
                }]
            }
            Some((gpending, gexpr)) => {
                let mut matched = bindings;
                matched.extend(gpending);
                matched.push(TirStmt::If {
                    cond: gexpr,
                    then_body,
                    else_body: else_body.clone(),
                });
                vec![TirStmt::If {
                    cond,
                    then_body: matched,
                    else_body,
                }]
            }
        }
    }

    fn match_pattern(&mut self, s: &TirExpr, pat: &MatchPattern) -> (TirExpr, Vec<TirStmt>) {
        match pat {
            MatchPattern::Wildcard => (bool_lit(true), vec![]),
            MatchPattern::Identifier(name) => {
                let name_str = self.m.interner.resolve(*name);
                if let BackendTy::Enum(eid) = s.ty.non_nullable(self.tt) {
                    if let Some(info) = self.m.enums.get(eid.0 as usize) {
                        if info.variants.iter().any(|v| v.name.as_ref() == name_str) {
                            return self.match_enum_variant(s, name_str, name_str, &[]);
                        }
                    }
                }
                let local = self.bind_local(Rc::from(name_str), s.ty);
                (
                    bool_lit(true),
                    vec![TirStmt::Let {
                        local,
                        ty: s.ty,
                        init: Some(s.clone()),
                    }],
                )
            }
            MatchPattern::Literal(lit) => {
                let l = self.lower_expr(*lit);
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
                let Some(cid) = self.m.names.class_id(self.m.interner.resolve(*type_name)) else {
                    return (bool_lit(false), vec![]);
                };
                let cond = TirExpr {
                    kind: TirExprKind::TypeTest {
                        value: Box::new(s.clone()),
                        class: cid,
                    },
                    ty: BackendTy::Bool,
                    res: Resolution::None,
                    span: s.span,
                };
                let mut binds = vec![];
                if let Some(name) = binding {
                    let local =
                        self.bind_local(Rc::from(self.m.interner.resolve(*name)), BackendTy::Class(cid));
                    binds.push(TirStmt::Let {
                        local,
                        ty: BackendTy::Class(cid),
                        init: Some(s.clone()),
                    });
                }
                (cond, binds)
            }
            MatchPattern::EnumVariant {
                enum_name,
                variant_name,
                bindings,
            } => {
                let enum_name = self.m.interner.resolve(*enum_name);
                let variant_name = self.m.interner.resolve(*variant_name);
                self.match_enum_variant(s, enum_name, variant_name, bindings)
            }

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
        let eid = self
            .m
            .names
            .enum_id(enum_name)
            .or_else(|| match s.ty.non_nullable(self.tt) {
                BackendTy::Enum(e) => Some(e),
                _ => None,
            });

        let Some(eid) = eid else {
            return self.match_variant_by_name(s, variant_name, bindings);
        };
        let Some(info) = self.m.enums.get(eid.0 as usize) else {
            return self.match_variant_by_name(s, variant_name, bindings);
        };
        let Some(variant) = info
            .variants
            .iter()
            .find(|v| v.name.as_ref() == variant_name)
        else {
            return self.match_variant_by_name(s, variant_name, bindings);
        };
        let tag = variant.tag;
        let payload: Vec<BackendTy> = variant.payload.clone();

        let disc = TirExpr {
            kind: TirExprKind::Discriminant {
                value: Box::new(s.clone()),
            },
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
            let fty = payload
                .get(i)
                .copied()
                .unwrap_or(BackendTy::Dynamic(DynReason::Unannotated));
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
            let local = self.bind_local(Rc::from(self.m.interner.resolve(b.name)), fty);
            binds.push(TirStmt::Let {
                local,
                ty: fty,
                init: Some(field),
            });
        }
        (cond, binds)
    }

    fn match_variant_by_name(
        &mut self,
        s: &TirExpr,
        variant_name: &str,
        bindings: &[MatchBinding],
    ) -> (TirExpr, Vec<TirStmt>) {
        let dyn_ty = BackendTy::Dynamic(DynReason::Unannotated);
        let by_name = |n: &str| Resolution::ByName {
            name: Rc::from(n),
            why: DynReason::Unannotated,
        };
        let field = |recv: TirExpr, name: &str, ty: BackendTy| TirExpr {
            kind: TirExprKind::Field {
                object: Box::new(recv),
                name: Rc::from(name),
            },
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
            let local = self.bind_local(Rc::from(self.m.interner.resolve(b.name)), dyn_ty);
            binds.push(TirStmt::Let {
                local,
                ty: dyn_ty,
                init: Some(init),
            });
        }
        (cond, binds)
    }

    fn lower_decl_stmt(&mut self, decl: &varn_core::ast::Decl) -> Vec<TirStmt> {
        use varn_core::ast::{Decl, ExportDecl};
        let unwrapped = match decl {
            Decl::Export(ExportDecl::Decl { declaration, .. }) => declaration.as_ref(),
            other => other,
        };

        if let Decl::Function(f) = unwrapped {
            let dyn_ty = BackendTy::Dynamic(DynReason::Unannotated);
            let local = self.bind_local(Rc::from(self.m.interner.resolve(f.id)), dyn_ty);
            let closure = self.lower_closure(
                &f.params,
                ClosureBody::Stmt(f.body),
                f.modifiers.is_async,
                f.modifiers.is_generator,
                dyn_ty,
                Span::EMPTY,
            );
            return vec![TirStmt::Let {
                local,
                ty: dyn_ty,
                init: Some(closure),
            }];
        }

        if let Decl::Namespace(ns) = unwrapped {
            let dyn_ty = BackendTy::Dynamic(DynReason::Unannotated);
            let local = self.bind_local(Rc::from(self.m.interner.resolve(ns.id)), dyn_ty);
            let mut entries: Vec<TirObjectEntry> = Vec::new();
            for m in &ns.body {
                let Decl::Export(ExportDecl::Decl { declaration, .. }) = m else {
                    continue;
                };
                if let Decl::Function(f) = declaration.as_ref() {
                    let closure = self.lower_closure(
                        &f.params,
                        ClosureBody::Stmt(f.body),
                        f.modifiers.is_async,
                        f.modifiers.is_generator,
                        dyn_ty,
                        Span::EMPTY,
                    );
                    entries.push(TirObjectEntry::Field {
                        name: Rc::from(self.m.interner.resolve(f.id)),
                        value: closure,
                    });
                }
            }
            let obj = TirExpr {
                kind: TirExprKind::ObjectLit { entries },
                ty: dyn_ty,
                res: Resolution::None,
                span: Span::EMPTY,
            };
            return vec![TirStmt::Let {
                local,
                ty: dyn_ty,
                init: Some(obj),
            }];
        }
        let v = match decl {
            Decl::Variable(v) => v,
            Decl::Export(ExportDecl::Decl { declaration, .. }) => match declaration.as_ref() {
                Decl::Variable(v) => v,
                _ => return vec![],
            },
            _ => return vec![],
        };
        let mut out = Vec::new();
        for d in &v.declarators {
            match &d.id {
                Pattern::Identifier { name, .. } => {
                    let name_str = self.m.interner.resolve(*name);
                    if let Some(init) = d.init {
                        if let ExprKind::Match { subject, cases } = &self.ast_arena.expr(init).kind {
                            let (subject, cases) = (*subject, cases);
                            let ty = self
                                .expr_table
                                .get(&init.index())
                                .map(|e| {
                                    let names = self.m.names;
                                    let table = self.m.checker_table;
                                    let interner = self.m.interner;
                                    lower_type(&e.ty, table, interner, self.tt, names)
                                })
                                .unwrap_or(BackendTy::Dynamic(DynReason::Unannotated));
                            let local = self.bind_local(Rc::from(name_str), ty);
                            out.push(TirStmt::Let {
                                local,
                                ty,
                                init: None,
                            });
                            out.extend(std::mem::take(&mut self.pending));
                            out.extend(self.lower_match(subject, cases, MatchDest::Assign(local)));
                            continue;
                        }
                    }

                    let prebound = {
                        let is_closure = d.init.is_some_and(|e| {
                            matches!(
                                self.ast_arena.expr(e).kind,
                                ExprKind::Arrow { .. } | ExprKind::Function { .. }
                            )
                        });
                        let is_global =
                            self.top_level && self.m.globals.contains_key(name_str);
                        if is_closure && !is_global {
                            Some(self.bind_local(
                                Rc::from(name_str),
                                BackendTy::Dynamic(DynReason::Unannotated),
                            ))
                        } else {
                            None
                        }
                    };

                    let init = d.init.map(|e| self.lower_expr(e));

                    let ty = d
                        .type_ann
                        .as_ref()
                        .map(|t| {
                            // Scratch table: see the matching comment in
                            // `emit/mod.rs`'s synthetic-constructor lowering —
                            // `ctx=None` can't resolve names, so these ids never
                            // need to match the module's real `CheckerTyTable`.
                            let mut scratch = crate::types::CheckerTyTable::new();
                            let resolved =
                                crate::binder::resolve_type_node(t, None, &mut scratch);
                            lower_type(&resolved, &scratch, self.m.interner, self.tt, self.m.names)
                        })
                        .filter(|t| !matches!(t, BackendTy::Dynamic(_)))
                        .or_else(|| init.as_ref().map(|e| e.ty))
                        .unwrap_or(BackendTy::Dynamic(DynReason::Unannotated));
                    out.extend(std::mem::take(&mut self.pending));

                    if self.top_level && prebound.is_none() {
                        if let Some(&slot) = self.m.globals.get(name_str) {
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

                    let local = prebound.unwrap_or_else(|| self.bind_local(Rc::from(name_str), ty));
                    out.push(TirStmt::Let { local, ty, init });
                }

                pat => {
                    let src = match d.init {
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

    pub fn destructure_params(&mut self, params: &[varn_core::ast::Param]) -> Vec<TirStmt> {
        let mut out = Vec::new();
        for (i, p) in params.iter().enumerate() {
            if let Some(def) = p.default {
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

    fn bind_pattern(&mut self, pat: &Pattern, src: TirExpr, out: &mut Vec<TirStmt>) {
        match pat {
            Pattern::Identifier { name, .. } => {
                let local = self.bind_local(Rc::from(self.m.interner.resolve(*name)), src.ty);
                out.push(TirStmt::Let {
                    local,
                    ty: src.ty,
                    init: Some(src),
                });
            }
            Pattern::Object {
                properties, rest, ..
            } => {
                for prop in properties {
                    let field = self.field_access(
                        src.clone(),
                        Rc::from(self.m.interner.resolve(prop.key)),
                        BackendTy::Dynamic(DynReason::Unannotated),
                        src.span,
                    );
                    self.bind_pattern(&prop.value, field, out);
                }
                if let Some(rest_pat) = rest {
                    let skip: Vec<Rc<str>> = properties
                        .iter()
                        .map(|p| Rc::from(self.m.interner.resolve(p.key)))
                        .collect();
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
                let elem_ty = match src.ty.non_nullable(self.tt) {
                    BackendTy::Array(e) => self.tt.get(e),
                    _ => BackendTy::Dynamic(DynReason::Unannotated),
                };
                for (i, slot) in elements.iter().enumerate() {
                    let Some(el) = slot else { continue };

                    let read_ty = if matches!(el.pattern, Pattern::Assignment { .. }) {
                        BackendTy::Dynamic(DynReason::Unannotated)
                    } else {
                        elem_ty
                    };
                    let idx = TirExpr {
                        kind: TirExprKind::Index {
                            object: Box::new(src.clone()),
                            index: Box::new(int_lit(i as i64)),
                        },
                        ty: read_ty,
                        res: Resolution::None,
                        span: src.span,
                    };
                    self.bind_pattern(&el.pattern, idx, out);
                }

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
                let def = self.lower_expr(*right);
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
                    src.clone()
                };
                self.bind_pattern(left, value, out);
            }

            Pattern::Rest { argument, .. } => self.bind_pattern(argument, src, out),
        }
    }

    fn lower_cond(&mut self, e: ExprId) -> TirExpr {
        let lowered = self.lower_expr(e);
        match lowered.ty {
            BackendTy::Bool | BackendTy::Dynamic(_) => lowered,
            _ => self.cast_to(lowered, BackendTy::Bool),
        }
    }

    fn cast_to(&self, e: TirExpr, ty: BackendTy) -> TirExpr {
        if e.ty == ty {
            return e;
        }
        let span = e.span;
        TirExpr {
            kind: TirExprKind::Cast {
                operand: Box::new(e),
            },
            ty,
            res: Resolution::None,
            span,
        }
    }

    fn lower_expr(&mut self, e: ExprId) -> TirExpr {
        let ty = self.expr_ty(e);
        let span = span_of(self.ast_arena, e);

        let kind = match &self.ast_arena.expr(e).kind {
            ExprKind::IntLiteral { value, .. } => Some(TirExprKind::IntLit(*value)),
            ExprKind::FloatLiteral { value, .. } => Some(TirExprKind::FloatLit(*value)),
            ExprKind::BoolLiteral { value } => Some(TirExprKind::BoolLit(*value)),
            ExprKind::StrLiteral { value } => Some(TirExprKind::StrLit(Rc::from(value.as_str()))),
            ExprKind::CharLiteral { value } => Some(TirExprKind::CharLit(*value)),
            ExprKind::NullLiteral => Some(TirExprKind::NullLit),

            ExprKind::Identifier { name } => {
                let name_str = self.m.interner.resolve(*name);
                return TirExpr {
                    kind: TirExprKind::Var,
                    ty,
                    res: self.resolve_name(name_str),
                    span,
                }
            }

            ExprKind::Paren { expression } => return self.lower_expr(*expression),

            ExprKind::Binary { op, left, right } => {
                return self.lower_binary(*op, *left, *right, ty, span)
            }
            ExprKind::Unary {
                op,
                operand,
                prefix: _,
            } => return self.lower_unary(*op, *operand, ty, span),

            ExprKind::This => {
                let this_ty = self
                    .this_enum
                    .map(BackendTy::Enum)
                    .or_else(|| self.this_class.map(BackendTy::Class))
                    .unwrap_or(BackendTy::Dynamic(DynReason::Unannotated));
                return TirExpr {
                    kind: TirExprKind::Var,
                    ty: this_ty,
                    res: Resolution::None,
                    span,
                };
            }

            ExprKind::Member {
                object,
                property,
                computed,
                optional,
            } => return self.lower_member(*object, *property, *computed, *optional, ty, span),

            ExprKind::Logical { op, left, right } => {
                return self.lower_logical(*op, *left, *right, ty, span)
            }

            ExprKind::Template { parts } => return self.lower_template(parts, span),

            ExprKind::Match { subject, cases } => {
                let subject = *subject;
                let result = self.fresh_local(ty);
                self.pending.push(TirStmt::Let {
                    local: result,
                    ty,
                    init: None,
                });
                let chain = self.lower_match(subject, cases, MatchDest::Assign(result));
                self.pending.extend(chain);
                return TirExpr {
                    kind: TirExprKind::Var,
                    ty,
                    res: Resolution::Local(result),
                    span,
                };
            }

            ExprKind::Function {
                params,
                body,
                is_async,
                is_generator,
                ..
            } => {
                return self.lower_closure(
                    params,
                    ClosureBody::Stmt(*body),
                    *is_async,
                    *is_generator,
                    ty,
                    span,
                )
            }
            ExprKind::Arrow {
                params,
                body,
                is_async,
                ..
            } => {
                let cb = match body.as_ref() {
                    varn_core::ast::ArrowBody::Expr(e) => ClosureBody::Expr(*e),
                    varn_core::ast::ArrowBody::Block(s) => ClosureBody::Stmt(*s),
                };
                return self.lower_closure(params, cb, *is_async, false, ty, span);
            }

            ExprKind::Await { argument } => {
                let fut = self.lower_expr(*argument);
                return TirExpr {
                    kind: TirExprKind::Await {
                        future: Box::new(fut),
                    },
                    ty,
                    res: Resolution::None,
                    span,
                };
            }
            ExprKind::Yield { argument, delegate } => {
                let value = argument.map(|a| Box::new(self.lower_expr(a)));
                return TirExpr {
                    kind: TirExprKind::Yield {
                        value,
                        delegate: *delegate,
                    },

                    ty: BackendTy::Dynamic(DynReason::Unannotated),
                    res: Resolution::None,
                    span,
                };
            }

            ExprKind::Call {
                callee,
                args,
                optional: _,
                type_args: _,
            } => {
                let callee = *callee;
                return self.lower_call(e.index(), callee, args, ty, span);
            }

            ExprKind::Array { elements } => {
                let els = elements
                    .iter()
                    .map(|el| match el {
                        ArrayEl::Expr(e) => TirArrayEl::Expr(self.lower_expr(*e)),
                        ArrayEl::Spread(e) => TirArrayEl::Spread(self.lower_expr(*e)),
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
                let xs = elements.iter().map(|&e| self.lower_expr(e)).collect();
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
                            Some((prop_key_name(key)?, self.lower_expr(*value)))
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
                let mut entries: Vec<TirObjectEntry> = Vec::new();
                for p in properties {
                    match p {
                        ObjectProp::Property { key, value, .. } => {
                            if let Some(name) = prop_key_name(key) {
                                entries.push(TirObjectEntry::Field {
                                    name,
                                    value: self.lower_expr(*value),
                                });
                            }
                        }
                        ObjectProp::Spread { argument, .. } => {
                            entries.push(TirObjectEntry::Spread(self.lower_expr(*argument)));
                        }

                        ObjectProp::Method {
                            key,
                            params,
                            body,
                            is_async,
                            is_generator,
                            ..
                        } => {
                            if let Some(name) = prop_key_name(key) {
                                let closure = self.lower_closure(
                                    params,
                                    ClosureBody::Stmt(*body),
                                    *is_async,
                                    *is_generator,
                                    BackendTy::Dynamic(DynReason::Unannotated),
                                    span,
                                );
                                entries.push(TirObjectEntry::Field {
                                    name,
                                    value: closure,
                                });
                            }
                        }

                        _ => {}
                    }
                }
                return TirExpr {
                    kind: TirExprKind::ObjectLit { entries },
                    ty,
                    res: Resolution::None,
                    span,
                };
            }
            ExprKind::New { callee, args, .. } => {
                let callee = *callee;
                return self.lower_new(e.index(), callee, args, ty, span);
            }

            ExprKind::NonNull { expression } => {
                let inner = self.lower_expr(*expression);
                let nn = inner.ty.non_nullable(self.tt);
                if inner.ty == nn {
                    return inner;
                }
                return TirExpr {
                    kind: TirExprKind::Cast {
                        operand: Box::new(inner),
                    },
                    ty: nn,
                    res: Resolution::None,
                    span,
                };
            }

            ExprKind::As { expression, .. } => {
                let inner = self.lower_expr(*expression);
                // `enumVal as int` means "this variant's raw value", not "the
                // bits of the reference reinterpreted as an int" — the latter
                // is what a bare `Cast` gives (it compiles to a `Move`,
                // `ssa/emit/values.rs`), and a `Ref`-classed operand has no
                // int bit pattern to reinterpret in the first place (the VM
                // rejects the store outright once the destination register
                // is genuinely `Gpr`). `.rawValue` is already how the
                // language reads this value explicitly (`Status.Success.
                // rawValue`); route the cast through the same lookup instead
                // of inventing a second, narrower path to the same field.
                if matches!(inner.ty, BackendTy::Enum(_)) && matches!(ty, BackendTy::Int) {
                    return self.field_access(inner, Rc::from("rawValue"), ty, span);
                }
                return TirExpr {
                    kind: TirExprKind::Cast {
                        operand: Box::new(inner),
                    },
                    ty,
                    res: Resolution::None,
                    span,
                };
            }
            ExprKind::Satisfies { expression, .. } => return self.lower_expr(*expression),

            ExprKind::Sequence { expressions } => {
                let Some((&last, lead)) = expressions.split_last() else {
                    return TirExpr {
                        kind: TirExprKind::NullLit,
                        ty: BackendTy::Void,
                        res: Resolution::None,
                        span,
                    };
                };
                for &e in lead {
                    let te = self.lower_expr(e);
                    self.pending.push(TirStmt::Expr(te));
                }
                return self.lower_expr(last);
            }

            ExprKind::Pipeline { left, right } => {
                let (left, right) = (*left, *right);
                if let ExprKind::Call { callee, args, .. } = &self.ast_arena.expr(right).kind {
                    let (callee, args) = (*callee, args);
                    let interner = self.m.interner;
                    let has_placeholder = args.iter().any(|a| {
                        matches!(
                            a,
                            Arg::Positional(e) | Arg::Named { value: e, .. }
                                if matches!(&self.ast_arena.expr(*e).kind, ExprKind::Identifier { name } if interner.resolve(*name) == "_")
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
                                    if matches!(&self.ast_arena.expr(*e).kind, ExprKind::Identifier { name } if interner.resolve(*name) == "_") =>
                                {
                                    TirArg::Expr(piped.clone())
                                }
                                other => self.lower_arg(other),
                            })
                            .collect();
                        return TirExpr {
                            kind: TirExprKind::Call {
                                callee: Box::new(c),
                                args: targs,
                            },
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

            ExprKind::With { object, properties } => {
                let object = *object;
                let mut entries = vec![TirObjectEntry::Spread(self.lower_expr(object))];
                for p in properties {
                    if let ObjectProp::Property { key, value, .. } = p {
                        if let Some(name) = prop_key_name(key) {
                            entries.push(TirObjectEntry::Field {
                                name,
                                value: self.lower_expr(*value),
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

            ExprKind::Assign { op, target, value }
                if matches!(assign_bin_op(*op), Ok(None))
                    && matches!(&self.ast_arena.expr(*target).kind, ExprKind::Member { .. })
                    && self
                        .m
                        .ext_set_members
                        .contains_key(&self.ast_arena.expr(*target).range.start.offset) =>
            {
                let (target, value) = (*target, *value);
                let ExprKind::Member { object, .. } = &self.ast_arena.expr(target).kind else {
                    unreachable!()
                };
                let object = *object;
                let mangled = self.m.ext_set_members[&self.ast_arena.expr(target).range.start.offset]
                    .clone();
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
                    self.ast_arena.expr(*target).kind,
                    ExprKind::Identifier { .. } | ExprKind::Member { .. }
                ) && Self::is_pure(self.ast_arena, *target) =>
            {
                let (target, value) = (*target, *value);
                let t = self.lower_expr(target);
                let v = self.lower_expr(value);
                let rhs = match assign_bin_op(*op) {
                    Ok(None) => v,
                    Ok(Some(bop)) => {
                        let (lhs, rhs, nty) = self.coerce_binary_operands(bop, t.clone(), v, t.ty);
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
                            _ => (v, t.clone()),
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
                    kind: TirExprKind::Assign {
                        target: Box::new(t),
                        value: Box::new(rhs),
                    },
                    ty,
                    res: Resolution::None,
                    span,
                };
            }

            ExprKind::Update {
                op,
                operand,
                prefix,
            } if matches!(
                self.ast_arena.expr(*operand).kind,
                ExprKind::Identifier { .. } | ExprKind::Member { .. }
            ) && Self::is_pure(self.ast_arena, *operand) =>
            {
                use varn_core::ast::operators::UpdateOp;
                let operand = *operand;
                let t = self.lower_expr(operand);
                let bop = match op {
                    UpdateOp::Increment => TirBinOp::Add,
                    UpdateOp::Decrement => TirBinOp::Sub,
                };
                let step = if t.ty == BackendTy::Float {
                    self.cast_to(int_lit(1), BackendTy::Float)
                } else {
                    int_lit(1)
                };

                let old = if *prefix {
                    t.clone()
                } else {
                    self.hoist(t.clone())
                };
                let (lhs, rhs, nty) = self.coerce_binary_operands(bop, old.clone(), step, t.ty);
                let stepped = TirExpr {
                    kind: TirExprKind::Binary {
                        op: bop,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    },
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

                self.pending.push(TirStmt::Expr(assign));
                return old;
            }

            ExprKind::DecimalLiteral { raw } => {
                let text: Rc<str> = Rc::from(self.m.interner.resolve(*raw).trim_end_matches('d'));
                Some(TirExprKind::DecimalLit(text))
            }
            ExprKind::BigIntLiteral { raw } => {
                let s = self.m.interner.resolve(*raw).trim_end_matches('n').replace('_', "");
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

            ExprKind::Spawn { argument } => {
                let inner = self.lower_expr(*argument);
                return self.cast_to(inner, ty);
            }
            ExprKind::Range {
                start,
                end,
                inclusive,
            } => {
                let s = self.lower_expr(*start);
                let en = self.lower_expr(*end);
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

            ExprKind::Is {
                expression,
                type_ann,
            } => {
                let v = self.lower_expr(*expression);
                let bool_ty = BackendTy::Bool;

                if let varn_core::TypeKind::Named(n, _) = &type_ann.kind {
                    if let Some(class) = self.m.names.class_id(self.m.interner.resolve(*n)) {
                        return TirExpr {
                            kind: TirExprKind::TypeTest {
                                value: Box::new(v),
                                class,
                            },
                            ty: bool_ty,
                            res: Resolution::None,
                            span,
                        };
                    }
                }

                let tag_name: Option<&'static str> = match &type_ann.kind {
                    varn_core::TypeKind::Intrinsic(t) => Some(t.name()),
                    varn_core::TypeKind::Named(n, _) => {
                        varn_core::TypeTag::from_str(self.m.interner.resolve(*n)).map(|t| t.name())
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

                return self.cast_to(v, bool_ty);
            }

            ExprKind::MetaAccess { target, property } => {
                let obj = self.lower_expr(*target);
                let key: Rc<str> = Rc::from(format!("::{}", self.m.interner.resolve(*property)));
                return TirExpr {
                    kind: TirExprKind::Field {
                        object: Box::new(obj),
                        name: key.clone(),
                    },
                    ty,
                    res: Resolution::ByName {
                        name: key,
                        why: DynReason::Unannotated,
                    },
                    span,
                };
            }

            ExprKind::Super => {
                let sty = self
                    .this_class
                    .map(BackendTy::Class)
                    .unwrap_or(BackendTy::Dynamic(DynReason::Unannotated));
                return TirExpr {
                    kind: TirExprKind::Var,
                    ty: sty,
                    res: Resolution::None,
                    span,
                };
            }
            ExprKind::TaggedTemplate { tag, template } => {
                let (tag, template) = (*tag, *template);
                use varn_core::ast::TemplatePart;
                let ExprKind::Template { parts } = &self.ast_arena.expr(template).kind else {
                    return self.lower_expr(template);
                };

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
                            let v = self.lower_expr(*e);
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

                if let ExprKind::Member {
                    object,
                    property,
                    computed: false,
                    ..
                } = &self.ast_arena.expr(tag).kind
                {
                    let (object, property) = (*object, *property);
                    if let Some(name) = Self::member_name(self.ast_arena, property, self.m.interner) {
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
                let res = match &self.ast_arena.expr(tag).kind {
                    ExprKind::Identifier { name } => Resolution::ByName {
                        name: Rc::from(self.m.interner.resolve(*name)),
                        why: DynReason::Unannotated,
                    },
                    _ => Resolution::None,
                };
                return TirExpr {
                    kind: TirExprKind::Call {
                        callee: Box::new(callee),
                        args: all_args,
                    },
                    ty,
                    res,
                    span,
                };
            }

            ExprKind::Conditional {
                test,
                consequent,
                alternate,
            } => {
                let (test, consequent, alternate) = (*test, *consequent, *alternate);
                let cond = self.lower_expr(test);
                let cond = self.cast_to(cond, BackendTy::Bool);
                let then_val = self.lower_expr(consequent);
                let else_val = self.lower_expr(alternate);
                let (then_val, else_val) =
                    if then_val.ty == else_val.ty || matches!(ty, BackendTy::Dynamic(_)) {
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

            ExprKind::ClassExpr { .. } => {
                return TirExpr {
                    kind: TirExprKind::Var,
                    ty,
                    res: self.resolve_name("<anon>"),
                    span,
                };
            }

            ExprKind::Try { expression } => {
                let inner = self.lower_expr(*expression);
                let hoisted = self.hoist(inner);
                let span = hoisted.span;

                // Case 1: Enum (Result or Option)
                if let BackendTy::Enum(eid) = hoisted.ty.non_nullable(self.tt) {
                    if let Some(info) = self.m.enums.get(eid.0 as usize) {
                        let is_err_res = info.variants.iter().find(|v| v.name.as_ref() == "Err");
                        let is_ok_res = info.variants.iter().find(|v| v.name.as_ref() == "Ok");
                        if let (Some(err_var), Some(ok_var)) = (is_err_res, is_ok_res) {
                            let disc = TirExpr {
                                kind: TirExprKind::Discriminant {
                                    value: Box::new(hoisted.clone()),
                                },
                                ty: BackendTy::Int,
                                res: Resolution::None,
                                span,
                            };
                            let cond = TirExpr {
                                kind: TirExprKind::Binary {
                                    op: TirBinOp::Eq,
                                    lhs: Box::new(disc),
                                    rhs: Box::new(TirExpr {
                                        kind: TirExprKind::IntLit(err_var.tag as i64),
                                        ty: BackendTy::Int,
                                        res: Resolution::None,
                                        span,
                                    }),
                                },
                                ty: BackendTy::Bool,
                                res: Resolution::None,
                                span,
                            };
                            self.pending.push(TirStmt::If {
                                cond,
                                then_body: vec![TirStmt::Return(Some(hoisted.clone()))],
                                else_body: vec![],
                            });
                            return TirExpr {
                                kind: TirExprKind::VariantPayload {
                                    value: Box::new(hoisted),
                                    tag: ok_var.tag,
                                    field: 0,
                                },
                                ty,
                                res: Resolution::EnumVariant {
                                    enum_id: eid,
                                    tag: ok_var.tag,
                                },
                                span,
                            };
                        }

                        let is_none_opt = info.variants.iter().find(|v| v.name.as_ref() == "None");
                        let is_some_opt = info.variants.iter().find(|v| v.name.as_ref() == "Some");
                        if let (Some(none_var), Some(some_var)) = (is_none_opt, is_some_opt) {
                            let disc = TirExpr {
                                kind: TirExprKind::Discriminant {
                                    value: Box::new(hoisted.clone()),
                                },
                                ty: BackendTy::Int,
                                res: Resolution::None,
                                span,
                            };
                            let cond = TirExpr {
                                kind: TirExprKind::Binary {
                                    op: TirBinOp::Eq,
                                    lhs: Box::new(disc),
                                    rhs: Box::new(TirExpr {
                                        kind: TirExprKind::IntLit(none_var.tag as i64),
                                        ty: BackendTy::Int,
                                        res: Resolution::None,
                                        span,
                                    }),
                                },
                                ty: BackendTy::Bool,
                                res: Resolution::None,
                                span,
                            };
                            self.pending.push(TirStmt::If {
                                cond,
                                then_body: vec![TirStmt::Return(Some(hoisted.clone()))],
                                else_body: vec![],
                            });
                            return TirExpr {
                                kind: TirExprKind::VariantPayload {
                                    value: Box::new(hoisted),
                                    tag: some_var.tag,
                                    field: 0,
                                },
                                ty,
                                res: Resolution::EnumVariant {
                                    enum_id: eid,
                                    tag: some_var.tag,
                                },
                                span,
                            };
                        }
                    }
                }

                // Case 2: Nullable type (T?)
                if matches!(hoisted.ty, BackendTy::Nullable(_)) {
                    let null_ty = BackendTy::Nullable(self.tt.intern(BackendTy::Never));
                    let null_expr = TirExpr {
                        kind: TirExprKind::NullLit,
                        ty: null_ty,
                        res: Resolution::None,
                        span,
                    };
                    let cond = TirExpr {
                        kind: TirExprKind::Unary {
                            op: TirUnOp::IsNull,
                            operand: Box::new(hoisted.clone()),
                        },
                        ty: BackendTy::Bool,
                        res: Resolution::None,
                        span,
                    };
                    self.pending.push(TirStmt::If {
                        cond,
                        then_body: vec![TirStmt::Return(Some(null_expr))],
                        else_body: vec![],
                    });
                    let non_null_ty = hoisted.ty.non_nullable(self.tt);
                    return TirExpr {
                        kind: TirExprKind::Cast {
                            operand: Box::new(hoisted),
                        },
                        ty: non_null_ty,
                        res: Resolution::None,
                        span,
                    };
                }

                return hoisted;
            }

            _ => None,
        };

        match kind {
            Some(kind) => TirExpr {
                kind,
                ty,
                res: Resolution::None,
                span,
            },
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
        left: ExprId,
        right: ExprId,
        ty: BackendTy,
        span: Span,
    ) -> TirExpr {
        let lhs = self.lower_expr(left);
        let rhs = self.lower_expr(right);

        if op == BinaryOp::Eq || op == BinaryOp::NotEq {
            let is_null_expr = |e: &TirExpr| -> bool {
                matches!(e.kind, TirExprKind::NullLit)
                    || e.ty.non_nullable(self.tt) == BackendTy::Never
            };
            let l_null = is_null_expr(&lhs);
            let r_null = is_null_expr(&rhs);
            if l_null || r_null {
                let is_eq = op == BinaryOp::Eq;
                if l_null && r_null {
                    return TirExpr {
                        kind: TirExprKind::BoolLit(is_eq),
                        ty: BackendTy::Bool,
                        res: Resolution::None,
                        span,
                    };
                }
                let target = if r_null { lhs } else { rhs };
                let is_null = TirExpr {
                    kind: TirExprKind::Unary {
                        op: TirUnOp::IsNull,
                        operand: Box::new(target),
                    },
                    ty: BackendTy::Bool,
                    res: Resolution::None,
                    span,
                };
                if is_eq {
                    return is_null;
                }
                return TirExpr {
                    kind: TirExprKind::Unary {
                        op: TirUnOp::Not,
                        operand: Box::new(is_null),
                    },
                    ty: BackendTy::Bool,
                    res: Resolution::None,
                    span,
                };
            }
        }

        let Some(top) = bin_op(op) else {
            if op == BinaryOp::Instanceof {
                if let ExprKind::Identifier { name } = &self.ast_arena.expr(right).kind {
                    if let Some(class) = self.m.names.class_id(self.m.interner.resolve(*name)) {
                        return TirExpr {
                            kind: TirExprKind::TypeTest {
                                value: Box::new(lhs),
                                class,
                            },
                            ty: BackendTy::Bool,
                            res: Resolution::None,
                            span,
                        };
                    }

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
            kind: TirExprKind::Binary {
                op: top,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            },
            ty: node_ty,
            res: Resolution::None,
            span,
        }
    }

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
            let ty = if is_cmp {
                BackendTy::Bool
            } else {
                BackendTy::Dynamic(DynReason::Unannotated)
            };
            return (lhs, rhs, ty);
        }
        if is_cmp && (l == BackendTy::Never || r == BackendTy::Never) {
            return (lhs, rhs, BackendTy::Bool);
        }
        // Los ocho tipos numéricos angostos son, para efectos de qué OPCODE
        // aritmético usar, la misma categoría que `Int`/`Float`: el checker
        // ya lo decide así (`numeric_binary_type` en `binder/
        // type_inference.rs` trata Int/I8/I16/I32/U8/U16/U32/U64 como una
        // sola cosa, ídem Float/F32) — este lowering DEBE dar el mismo tipo
        // de resultado, o `i8 + i16` cae en el `else { l }` de más abajo y
        // castea SILENCIOSAMENTE el operando derecho al ancho del IZQUIERDO
        // (`i16(1000) as i8` sin que el usuario lo haya pedido), reventando
        // en runtime con "1000 no cabe en i8" sobre una suma que el checker
        // ya había tipado como `int` sin restricción de ancho. Angostar el
        // RESULTADO de vuelta sigue exigiendo el cast explícito de siempre
        // (`(a + b) as i8`), que es donde el rango se verifica de verdad.
        fn narrow_int_widened(bt: BackendTy) -> Option<BackendTy> {
            matches!(
                bt,
                BackendTy::Int8
                    | BackendTy::Int16
                    | BackendTy::Int32
                    | BackendTy::UInt8
                    | BackendTy::UInt16
                    | BackendTy::UInt32
            )
            .then_some(BackendTy::Int)
        }
        fn narrow_float_widened(bt: BackendTy) -> Option<BackendTy> {
            matches!(bt, BackendTy::Float32).then_some(BackendTy::Float)
        }
        let l = narrow_int_widened(l)
            .or_else(|| narrow_float_widened(l))
            .unwrap_or(l);
        let r = narrow_int_widened(r)
            .or_else(|| narrow_float_widened(r))
            .unwrap_or(r);

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

        // `+` on a string operand is concatenation regardless of what the
        // other side is (matches `infer_binary_type`'s checking-phase rule) —
        // this must be checked before the `Float` arm below, or `str + float`
        // resolves to `Float` and casts the string literal to a float. The
        // typed `AddFloat` opcode that produces trusts its operands
        // unconditionally (unlike the interpreter's `AddFloat`, which falls
        // back to generic `add` on a non-numeric operand), so that mistyping
        // wasn't just slow — it silently corrupted the value.
        let common = if op == TirBinOp::Add
            && (matches!(l, BackendTy::Str) || matches!(r, BackendTy::Str))
        {
            BackendTy::Str
        } else if l == BackendTy::Float || r == BackendTy::Float {
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

    fn member_name(ast_arena: &AstArena, property: ExprId, interner: &AtomInterner) -> Option<Rc<str>> {
        match &ast_arena.expr(property).kind {
            ExprKind::Identifier { name } => Some(Rc::from(interner.resolve(*name))),
            ExprKind::StrLiteral { value } => Some(Rc::from(value.as_str())),
            _ => None,
        }
    }

    fn is_pure(ast_arena: &AstArena, e: ExprId) -> bool {
        match &ast_arena.expr(e).kind {
            ExprKind::Identifier { .. }
            | ExprKind::This
            | ExprKind::IntLiteral { .. }
            | ExprKind::FloatLiteral { .. }
            | ExprKind::BoolLiteral { .. }
            | ExprKind::StrLiteral { .. }
            | ExprKind::CharLiteral { .. }
            | ExprKind::NullLiteral => true,
            ExprKind::Paren { expression } => Self::is_pure(ast_arena, *expression),
            ExprKind::Member {
                object,
                property,
                computed,
                ..
            } => {
                Self::is_pure(ast_arena, *object)
                    && (!computed || Self::is_pure(ast_arena, *property))
            }
            ExprKind::Binary { left, right, .. } => {
                Self::is_pure(ast_arena, *left) && Self::is_pure(ast_arena, *right)
            }
            ExprKind::Unary { operand, .. } => Self::is_pure(ast_arena, *operand),
            _ => false,
        }
    }

    fn lower_logical(
        &mut self,
        op: LogicalOp,
        left: ExprId,
        right: ExprId,
        ty: BackendTy,
        span: Span,
    ) -> TirExpr {
        let (cond, then_val, else_val) = match op {
            LogicalOp::And | LogicalOp::Or => {
                let l = self.lower_expr(left);
                let l = self.cast_to(l, BackendTy::Bool);
                let r = self.lower_expr(right);
                let r = self.cast_to(r, BackendTy::Bool);
                match op {
                    LogicalOp::And => (l, r, bool_lit(false)),
                    _ => (l, bool_lit(true), r),
                }
            }
            LogicalOp::Nullish => {
                let mut l = self.lower_expr(left);
                if !Self::is_pure(self.ast_arena, left) {
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
                (is_null, r, l)
            }
        };

        let (then_val, else_val) =
            if then_val.ty == else_val.ty || matches!(ty, BackendTy::Dynamic(_)) {
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
        object: ExprId,
        property: ExprId,
        computed: bool,
        optional: bool,
        ty: BackendTy,
        span: Span,
    ) -> TirExpr {
        if optional && !computed {
            let name = Self::member_name(self.ast_arena, property, self.m.interner)
                .unwrap_or_else(|| Rc::from("<member>"));
            let mut recv = self.lower_expr(object);
            if !Self::is_pure(self.ast_arena, object) {
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
            if let ExprKind::Range {
                start,
                end,
                inclusive,
            } = &self.ast_arena.expr(property).kind
            {
                let (start, end, inclusive) = (*start, *end, *inclusive);
                let s = self.lower_expr(start);
                let mut e = self.lower_expr(end);
                if inclusive {
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

            let index = self.lower_expr(property);
            let node_ty = match obj.ty.non_nullable(self.tt) {
                BackendTy::Array(el) => self.tt.get(el),
                BackendTy::Map(_, val) => self.tt.get(val),
                _ => ty,
            };
            return TirExpr {
                kind: TirExprKind::Index {
                    object: Box::new(obj),
                    index: Box::new(index),
                },
                ty: node_ty,
                res: Resolution::None,
                span,
            };
        }

        let name = Self::member_name(self.ast_arena, property, self.m.interner)
            .unwrap_or_else(|| Rc::from("<member>"));

        if let Some(mangled) = self
            .m
            .ext_members
            .get(&self.ast_arena.expr(property).range.start.offset)
            .cloned()
        {
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

    fn field_access(&mut self, obj: TirExpr, name: Rc<str>, ty: BackendTy, span: Span) -> TirExpr {
        match self
            .class_of(obj.ty)
            .and_then(|ci| ci.field(&name).cloned())
        {
            Some(field) => TirExpr {
                kind: TirExprKind::Field {
                    object: Box::new(obj),
                    name,
                },
                ty: field.ty,
                res: Resolution::FieldSlot(field.slot),
                span,
            },
            None => TirExpr {
                kind: TirExprKind::Field {
                    object: Box::new(obj),
                    name: name.clone(),
                },
                ty,
                res: Resolution::ByName {
                    name,
                    why: DynReason::Unannotated,
                },
                span,
            },
        }
    }

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

        let param_names: Vec<Rc<str>> = params
            .iter()
            .map(|p| pattern_lead(&p.pattern, self.m.interner))
            .collect();
        let param_tys = vec![BackendTy::Dynamic(DynReason::Unannotated); params.len()];

        let mut outer_names = self.outer_names.clone();
        for scope in &self.scopes {
            outer_names.extend(scope.keys().cloned());
        }
        outer_names.extend(self.params.iter().cloned());

        let mut sub = FnEmitter::new(
            self.ast_arena,
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

        let upvalues: Vec<varn_tir::TirUpvalue> = captures
            .iter()
            .map(|name| match self.resolve_name(name) {
                Resolution::Local(id) => varn_tir::TirUpvalue::ParentLocal(id.0),
                Resolution::Param(i) => varn_tir::TirUpvalue::ParentParam(i),
                Resolution::Upvalue(i) => varn_tir::TirUpvalue::ParentUpvalue(i),

                _ => varn_tir::TirUpvalue::ParentUpvalue(0),
            })
            .collect();

        TirExpr {
            kind: TirExprKind::Closure {
                func: func_id,
                upvalues,
            },
            ty,
            res: Resolution::None,
            span,
        }
    }

    fn lower_template(&mut self, parts: &[varn_core::ast::TemplatePart], span: Span) -> TirExpr {
        use varn_core::ast::TemplatePart;
        let str_expr = |kind, span| TirExpr {
            kind,
            ty: BackendTy::Str,
            res: Resolution::None,
            span,
        };
        let mut acc: Option<TirExpr> = None;
        for part in parts {
            let piece = match part {
                TemplatePart::Literal(s) => {
                    str_expr(TirExprKind::StrLit(Rc::from(s.as_str())), span)
                }
                TemplatePart::Interpolation(e) => {
                    let le = self.lower_expr(*e);
                    if le.ty == BackendTy::Str {
                        le
                    } else {
                        str_expr(
                            TirExprKind::Cast {
                                operand: Box::new(le),
                            },
                            span,
                        )
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

    fn lower_new(
        &mut self,
        call_id: AstId,
        callee: ExprId,
        args: &[Arg],
        ty: BackendTy,
        span: Span,
    ) -> TirExpr {
        let class = match &self.ast_arena.expr(callee).kind {
            ExprKind::Identifier { name } => self.m.names.class_id(self.m.interner.resolve(*name)),

            ExprKind::Member {
                property,
                computed: false,
                ..
            } => Self::member_name(self.ast_arena, *property, self.m.interner)
                .and_then(|n| self.m.names.class_id(&n)),
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

            None => {
                let c = self.lower_expr(callee);
                TirExpr {
                    kind: TirExprKind::Call {
                        callee: Box::new(c),
                        args: targs,
                    },
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

    fn enum_variant(&self, object: ExprId, variant: &str) -> Option<(varn_tir::EnumId, u16)> {
        let ExprKind::Identifier { name } = &self.ast_arena.expr(object).kind else {
            return None;
        };
        let eid = self.m.names.enum_id(self.m.interner.resolve(*name))?;
        let info = self.m.enums.get(eid.0 as usize)?;
        let v = info.variants.iter().find(|v| v.name.as_ref() == variant)?;
        Some((eid, v.tag))
    }

    fn lower_arg(&mut self, a: &Arg) -> TirArg {
        match a {
            Arg::Positional(e) => TirArg::Expr(self.lower_expr(*e)),
            Arg::Spread(e) => TirArg::Spread(self.lower_expr(*e)),
            Arg::Named { label, value } => TirArg::Named {
                label: Rc::from(label.as_str()),
                value: self.lower_expr(*value),
            },
        }
    }

    fn lower_call_args(&mut self, call_id: AstId, args: &[Arg]) -> Vec<TirArg> {
        match self.m.call_mappings.get(&call_id).cloned() {
            Some(mapping) => mapping
                .iter()
                .map(|opt| match opt {
                    Some(i) => match &args[*i] {
                        Arg::Positional(e) | Arg::Named { value: e, .. } => {
                            TirArg::Expr(self.lower_expr(*e))
                        }
                        Arg::Spread(e) => TirArg::Spread(self.lower_expr(*e)),
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
        callee: ExprId,
        args: &[Arg],
        ty: BackendTy,
        span: Span,
    ) -> TirExpr {
        if matches!(self.ast_arena.expr(callee).kind, ExprKind::Super) {
            let targs = self.lower_call_args(call_id, args);
            return TirExpr {
                kind: TirExprKind::SuperCall { args: targs },
                ty,
                res: Resolution::None,
                span,
            };
        }

        if let ExprKind::Member {
            object,
            property,
            computed: false,
            ..
        } = &self.ast_arena.expr(callee).kind
        {
            let (object, property) = (*object, *property);
            if matches!(self.ast_arena.expr(object).kind, ExprKind::Super) {
                if let Some(name) = Self::member_name(self.ast_arena, property, self.m.interner) {
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

        if let ExprKind::Identifier { name } = &self.ast_arena.expr(callee).kind {
            let c = self.lower_expr(callee);
            let targs = self.lower_call_args(call_id, args);
            let all_positional = targs.iter().all(|a| matches!(a, TirArg::Expr(_)));
            let res = match self.m.fns.get(name) {
                Some(&(fn_id, arity)) if all_positional && arity as usize == targs.len() => {
                    Resolution::DirectFn(varn_tir::FnId(fn_id))
                }
                // A `std:math` import (`abs`, `sqrt`, …) the JIT lowers to a
                // single ISA instruction; the import binding rules out a shadow.
                _ if all_positional && self.m.math_intrinsics.contains_key(name) => {
                    Resolution::Intrinsic(self.m.math_intrinsics[name] as u16)
                }
                _ => match c.res {
                    Resolution::NativeGlobal(idx) => Resolution::NativeGlobal(idx),
                    _ => Resolution::ByName {
                        name: Rc::from(self.m.interner.resolve(*name)),
                        why: DynReason::Unannotated,
                    },
                },
            };
            return TirExpr {
                kind: TirExprKind::Call {
                    callee: Box::new(c),
                    args: targs,
                },
                ty,
                res,
                span,
            };
        }

        let (object, property) = match &self.ast_arena.expr(callee).kind {
            ExprKind::Member {
                object,
                property,
                computed: false,
                ..
            } => (*object, *property),
            _ => return self.by_name_call(call_id, callee, args, ty, span),
        };
        let Some(name) = Self::member_name(self.ast_arena, property, self.m.interner) else {
            return self.by_name_call(call_id, callee, args, ty, span);
        };

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

        let all_positional = targs.iter().all(|a| matches!(a, TirArg::Expr(_)));

        // Core-type instance method → direct op-id dispatch.
        if all_positional {
            if let Some(cls) = self.core_class_name(recv.ty) {
                if self.m.core_ops.contains(&(Rc::from(cls), Rc::clone(&name))) {
                    let op_id = varn_core::op_id::core_method_op_id(cls, &name);
                    return TirExpr {
                        kind: TirExprKind::MethodCall {
                            recv: Box::new(recv),
                            name,
                            args: targs,
                        },
                        ty,
                        res: Resolution::NativeOp(op_id),
                        span,
                    };
                }
            }
        }

        let res = self
            .class_of(recv.ty)
            .and_then(|ci| ci.method_slot(&name).map(|s| (s, ci)))
            .and_then(|(slot, ci)| {
                let entry = ci.method_at(slot)?;
                let sig = self.signatures.get(entry.sig.0 as usize)?;
                (all_positional && sig.params.len() == targs.len())
                    .then_some(Resolution::VtableSlot(slot))
            })
            .unwrap_or(Resolution::ByName {
                name: name.clone(),
                why: DynReason::Unannotated,
            });

        TirExpr {
            kind: TirExprKind::MethodCall {
                recv: Box::new(recv),
                name,
                args: targs,
            },
            ty,
            res,
            span,
        }
    }

    fn by_name_call(
        &mut self,
        call_id: AstId,
        callee: ExprId,
        args: &[Arg],
        ty: BackendTy,
        span: Span,
    ) -> TirExpr {
        let c = self.lower_expr(callee);
        let targs = self.lower_call_args(call_id, args);
        TirExpr {
            kind: TirExprKind::Call {
                callee: Box::new(c),
                args: targs,
            },
            ty,
            res: Resolution::ByName {
                name: Rc::from("<call>"),
                why: DynReason::Unannotated,
            },
            span,
        }
    }

    fn lower_unary(&mut self, op: UnaryOp, operand: ExprId, ty: BackendTy, span: Span) -> TirExpr {
        let top = match op {
            UnaryOp::Minus => TirUnOp::Neg,
            UnaryOp::Not => TirUnOp::Not,
            UnaryOp::BitNot => TirUnOp::BitNot,
            UnaryOp::Plus => return self.lower_expr(operand),

            UnaryOp::Typeof => {
                let inner = self.lower_expr(operand);
                return TirExpr {
                    kind: TirExprKind::Unary {
                        op: TirUnOp::Typeof,
                        operand: Box::new(inner),
                    },
                    ty: BackendTy::Str,
                    res: Resolution::None,
                    span,
                };
            }
        };

        let inner = self.lower_expr(operand);
        TirExpr {
            kind: TirExprKind::Unary {
                op: top,
                operand: Box::new(inner),
            },
            ty,
            res: Resolution::None,
            span,
        }
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

fn has_continue(ast_arena: &AstArena, stmt: StmtId) -> bool {
    fn check(ast_arena: &AstArena, stmt: StmtId, in_nested_loop: bool) -> bool {
        match &ast_arena.stmt(stmt).kind {
            StmtKind::Continue { label } => {
                if in_nested_loop {
                    label.is_some()
                } else {
                    true
                }
            }
            StmtKind::Block { stmts } => stmts.iter().any(|&s| check(ast_arena, s, in_nested_loop)),
            StmtKind::If {
                consequent,
                alternate,
                ..
            } => {
                check(ast_arena, *consequent, in_nested_loop)
                    || alternate.map_or(false, |a| check(ast_arena, a, in_nested_loop))
            }
            StmtKind::Switch { cases, .. } => cases
                .iter()
                .any(|c| c.body.iter().any(|&s| check(ast_arena, s, in_nested_loop))),
            StmtKind::Try {
                block,
                catches,
                finally,
            } => {
                check(ast_arena, *block, in_nested_loop)
                    || catches.iter().any(|c| check(ast_arena, c.body, in_nested_loop))
                    || finally.map_or(false, |f| check(ast_arena, f, in_nested_loop))
            }
            StmtKind::Labeled { body, .. } => check(ast_arena, *body, in_nested_loop),
            StmtKind::While { body, .. }
            | StmtKind::DoWhile { body, .. }
            | StmtKind::For { body, .. }
            | StmtKind::ForIn { body, .. }
            | StmtKind::ForOf { body, .. } => check(ast_arena, *body, true),
            _ => false,
        }
    }
    check(ast_arena, stmt, false)
}
