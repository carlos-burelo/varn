//! Function bodies.
//!
//! Sub-phase 2a: literals, `Identifier`, `let`, `return`, `if`, `while`,
//! `throw`, `break` / `continue`, expression statements, and the scalar-safe
//! subset of `Binary` / `Unary`. Everything else — calls, member access,
//! `match`, C-style `for`, `for…of` — lowers to a `Dynamic(NotYetSupported)`
//! placeholder, never a half-built node the verifier cannot check.

use crate::checker::TypeEntry;
use crate::emit::tables::NameIndex;
use crate::emit::ty::lower_type;
use rustc_hash::FxHashMap;
use std::rc::Rc;
use varn_core::ast::operators::{BinaryOp, UnaryOp};
use varn_core::ast::{Arg, AstId, Expr, ExprKind, Pattern, Stmt, StmtKind};
use varn_tir::{
    BackendTy, ClassId, ClassInfo, DynReason, LocalId, Resolution, Signature, Span, TirArg,
    TirBinOp, TirExpr, TirExprKind, TirStmt, TirUnOp, TyTable,
};

/// The module-wide handles a body emitter needs but does not own.
#[derive(Clone, Copy)]
pub(super) struct ModuleCtx<'a> {
    pub names: &'a NameIndex,
    pub classes: &'a [ClassInfo],
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
    pub locals: Vec<BackendTy>,
    scopes: Vec<FxHashMap<Rc<str>, LocalId>>,
    params: Vec<Rc<str>>,
    this_class: Option<ClassId>,
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
        params: Vec<Rc<str>>,
    ) -> Self {
        FnEmitter {
            expr_table,
            tt,
            m,
            signatures,
            locals: Vec::new(),
            scopes: vec![FxHashMap::default()],
            params,
            this_class: None,
        }
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

    fn resolve_name(&self, name: &str) -> Resolution {
        for scope in self.scopes.iter().rev() {
            if let Some(id) = scope.get(name) {
                return Resolution::Local(*id);
            }
        }
        if let Some(i) = self.params.iter().position(|p| p.as_ref() == name) {
            return Resolution::Param(i as u32);
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
        let out = stmts.iter().filter_map(|s| self.lower_stmt(s)).collect();
        self.scopes.pop();
        out
    }

    pub fn lower_stmt_as_block(&mut self, s: &Stmt) -> Vec<TirStmt> {
        match &s.kind {
            StmtKind::Block { stmts } => self.lower_block(stmts),
            _ => self.lower_stmt(s).into_iter().collect(),
        }
    }

    fn lower_stmt(&mut self, s: &Stmt) -> Option<TirStmt> {
        match &s.kind {
            StmtKind::Block { stmts } => {
                // A bare block keeps its own scope but has no TIR node of its
                // own; splice its statements. Rare at statement position.
                let inner = self.lower_block(stmts);
                Some(TirStmt::If {
                    cond: bool_lit(true),
                    then_body: inner,
                    else_body: vec![],
                })
            }
            StmtKind::Expr { expression } => Some(TirStmt::Expr(self.lower_expr(expression))),
            StmtKind::Empty | StmtKind::Debugger | StmtKind::Error => None,

            StmtKind::Decl(decl) => self.lower_decl_stmt(decl),

            StmtKind::Return { argument } => {
                Some(TirStmt::Return(argument.as_ref().map(|a| self.lower_expr(a))))
            }
            StmtKind::Throw { argument } => Some(TirStmt::Throw(self.lower_expr(argument))),
            StmtKind::Break { .. } => Some(TirStmt::Break),
            StmtKind::Continue { .. } => Some(TirStmt::Continue),

            StmtKind::If { test, consequent, alternate } => {
                let cond = self.lower_cond(test);
                let then_body = self.lower_stmt_as_block(consequent);
                let else_body =
                    alternate.as_ref().map(|a| self.lower_stmt_as_block(a)).unwrap_or_default();
                Some(TirStmt::If { cond, then_body, else_body })
            }

            StmtKind::While { test, body } => {
                let cond = self.lower_cond(test);
                let body = self.lower_stmt_as_block(body);
                Some(TirStmt::Loop { cond, body })
            }

            // C-style for, do-while, for-of/in, switch, try, using, labeled:
            // sub-phase 2b and later. Emit a placeholder statement so the
            // shape is visible in the dump and counted.
            _ => Some(TirStmt::Expr(placeholder(DynReason::NotYetSupported))),
        }
    }

    fn lower_decl_stmt(&mut self, decl: &varn_core::ast::Decl) -> Option<TirStmt> {
        use varn_core::ast::Decl;
        let Decl::Variable(v) = decl else {
            // Nested function/class/enum declarations are handled at module
            // level, not as body statements.
            return None;
        };
        // One `let` with several declarators becomes several `Let` statements;
        // only the last is returned, the rest are pushed. Simplest correct
        // lowering without a block node.
        let mut last = None;
        for d in &v.declarators {
            let Pattern::Identifier { name, .. } = &d.id else {
                // Destructuring: sub-phase later.
                continue;
            };
            let init = d.init.as_ref().map(|e| self.lower_expr(e));
            let ty = init
                .as_ref()
                .map(|e| e.ty)
                .unwrap_or(BackendTy::Dynamic(DynReason::Unannotated));
            let local = self.bind_local(name.clone(), ty);
            last = Some(TirStmt::Let { local, ty, init });
        }
        last
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

            ExprKind::Member { object, property, computed, optional: _ } => {
                return self.lower_member(object, property, *computed, ty, span)
            }

            ExprKind::Call { callee, args, optional: _, type_args: _ } => {
                return self.lower_call(callee, args, ty, span)
            }

            // Only a plain `=` to an identifier or a field. Compound assign
            // (`+=` …) and destructuring targets are later sub-phases.
            ExprKind::Assign { op: varn_core::ast::operators::AssignOp::Assign, target, value }
                if matches!(
                    target.kind,
                    ExprKind::Identifier { .. } | ExprKind::Member { .. }
                ) =>
            {
                let t = self.lower_expr(target);
                let v = self.lower_expr(value);
                return TirExpr {
                    kind: TirExprKind::Assign { target: Box::new(t), value: Box::new(v) },
                    ty,
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

    fn lower_member(
        &mut self,
        object: &Expr,
        property: &Expr,
        computed: bool,
        ty: BackendTy,
        span: Span,
    ) -> TirExpr {
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

        // A known field on a class receiver: the node type IS the field's
        // declared type — that is the authority, and it is what the verifier
        // checks `FieldSlot` against. The checker's type for the access
        // expression can be absent (an assignment target) or weaker, so it is
        // not used here. Anything not a known field is a by-name read (getter,
        // index signature, dynamic receiver).
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

fn bool_lit(v: bool) -> TirExpr {
    TirExpr {
        kind: TirExprKind::BoolLit(v),
        ty: BackendTy::Bool,
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
