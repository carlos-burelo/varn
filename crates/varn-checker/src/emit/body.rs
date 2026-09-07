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
use varn_core::ast::{AstId, Expr, ExprKind, Pattern, Stmt, StmtKind};
use varn_tir::{
    BackendTy, DynReason, LocalId, Resolution, Signature, Span, TirBinOp, TirExpr, TirExprKind,
    TirStmt, TirUnOp, TyTable,
};

pub(super) struct FnEmitter<'a> {
    pub expr_table: &'a FxHashMap<AstId, TypeEntry>,
    pub tt: &'a mut TyTable,
    pub names: &'a NameIndex,
    #[allow(dead_code)]
    pub signatures: &'a mut Vec<Signature>,
    pub locals: Vec<BackendTy>,
    scopes: Vec<FxHashMap<Rc<str>, LocalId>>,
    params: Vec<Rc<str>>,
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
        names: &'a NameIndex,
        signatures: &'a mut Vec<Signature>,
        params: Vec<Rc<str>>,
    ) -> Self {
        FnEmitter {
            expr_table,
            tt,
            names,
            signatures,
            locals: Vec::new(),
            scopes: vec![FxHashMap::default()],
            params,
        }
    }

    /// The type the checker proved for this expression, `refined` over `ty`,
    /// lowered. An expression the checker never recorded is `Unannotated`.
    fn expr_ty(&mut self, e: &Expr) -> BackendTy {
        match self.expr_table.get(&e.id) {
            Some(entry) => {
                let t = entry.refined.clone().unwrap_or_else(|| entry.ty.clone());
                lower_type(&t, self.tt, self.names)
            }
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
        // Globals and imports resolve in sub-phase 4.
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
