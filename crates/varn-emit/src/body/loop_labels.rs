use super::context::FnEmitter;
use std::collections::BTreeSet;
use varn_core::ast::{StmtId, StmtKind};
use varn_core::Atom;
use varn_tir::{BackendTy, LocalId, Resolution, Span, TirBinOp, TirExpr, TirExprKind, TirStmt};

const LEAVE: i64 = 1;
const AGAIN: i64 = 2;

pub(super) struct LoopFrame {
    label: Option<Atom>,
    flag: Option<LocalId>,
    crossed_by: BTreeSet<usize>,
}

fn int(value: i64) -> TirExpr {
    TirExpr {
        kind: TirExprKind::IntLit(value),
        ty: BackendTy::Int,
        res: Resolution::None,
        span: Span::EMPTY,
    }
}

fn flag_var(flag: LocalId) -> TirExpr {
    TirExpr {
        kind: TirExprKind::Var,
        ty: BackendTy::Int,
        res: Resolution::Local(flag),
        span: Span::EMPTY,
    }
}

fn set_flag(flag: LocalId, value: i64) -> TirStmt {
    TirStmt::Expr(TirExpr {
        kind: TirExprKind::Assign {
            target: Box::new(flag_var(flag)),
            value: Box::new(int(value)),
        },
        ty: BackendTy::Void,
        res: Resolution::None,
        span: Span::EMPTY,
    })
}

fn flag_is(flag: LocalId, op: TirBinOp, value: i64) -> TirExpr {
    TirExpr {
        kind: TirExprKind::Binary {
            op,
            lhs: Box::new(flag_var(flag)),
            rhs: Box::new(int(value)),
        },
        ty: BackendTy::Bool,
        res: Resolution::None,
        span: Span::EMPTY,
    }
}

impl<'a> FnEmitter<'a> {
    pub(super) fn lower_stmt(&mut self, s: StmtId) -> Vec<TirStmt> {
        match &self.ast_arena.stmt(s).kind {
            StmtKind::Labeled { label, body } => {
                self.pending_label = Some(*label);
                let lowered = self.lower_stmt_as_block(*body);
                self.pending_label = None;
                lowered
            }
            StmtKind::While { .. }
            | StmtKind::DoWhile { .. }
            | StmtKind::For { .. }
            | StmtKind::ForIn { .. }
            | StmtKind::ForOf { .. } => self.lower_loop(s),
            StmtKind::Break { label: Some(label) } => self.jump_to(*label, LEAVE),
            StmtKind::Continue { label: Some(label) } => self.jump_to(*label, AGAIN),
            StmtKind::Break { label: None }
            | StmtKind::Continue { label: None }
            | StmtKind::Block { .. }
            | StmtKind::Empty
            | StmtKind::Expr { .. }
            | StmtKind::Decl(_)
            | StmtKind::Error
            | StmtKind::If { .. }
            | StmtKind::Switch { .. }
            | StmtKind::Return { .. }
            | StmtKind::Throw { .. }
            | StmtKind::Try { .. }
            | StmtKind::Using { .. }
            | StmtKind::Debugger => self.lower_plain_stmt(s),
        }
    }

    fn lower_loop(&mut self, s: StmtId) -> Vec<TirStmt> {
        self.loops.push(LoopFrame {
            label: self.pending_label.take(),
            flag: None,
            crossed_by: BTreeSet::new(),
        });
        let mut out = self.lower_plain_stmt(s);
        let Some(frame) = self.loops.pop() else {
            return out;
        };
        if let Some(flag) = frame.flag {
            out.insert(
                0,
                TirStmt::Let {
                    local: flag,
                    ty: BackendTy::Int,
                    init: Some(int(0)),
                },
            );
        }
        let around = self.loops.len();
        for target in frame.crossed_by {
            let Some(flag) = self.loops.get(target).and_then(|f| f.flag) else {
                continue;
            };
            if target + 1 == around {
                out.push(TirStmt::If {
                    cond: flag_is(flag, TirBinOp::Eq, LEAVE),
                    then_body: vec![set_flag(flag, 0), TirStmt::Break],
                    else_body: vec![],
                });
                out.push(TirStmt::If {
                    cond: flag_is(flag, TirBinOp::Eq, AGAIN),
                    then_body: vec![set_flag(flag, 0), TirStmt::Continue],
                    else_body: vec![],
                });
            } else {
                out.push(TirStmt::If {
                    cond: flag_is(flag, TirBinOp::Ne, 0),
                    then_body: vec![TirStmt::Break],
                    else_body: vec![],
                });
            }
        }
        out
    }

    fn jump_to(&mut self, label: Atom, how: i64) -> Vec<TirStmt> {
        let direct = if how == LEAVE {
            TirStmt::Break
        } else {
            TirStmt::Continue
        };
        let Some(innermost) = self.loops.len().checked_sub(1) else {
            return vec![direct];
        };
        let Some(target) = self.loops.iter().rposition(|f| f.label == Some(label)) else {
            return vec![direct];
        };
        if target == innermost {
            return vec![direct];
        }
        let flag = match self.loops[target].flag {
            Some(flag) => flag,
            None => {
                let flag = self.fresh_local(BackendTy::Int);
                self.loops[target].flag = Some(flag);
                flag
            }
        };
        for frame in &mut self.loops[target + 1..] {
            frame.crossed_by.insert(target);
        }
        vec![set_flag(flag, how), TirStmt::Break]
    }
}
