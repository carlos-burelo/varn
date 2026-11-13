use crate::children::child_exprs;
use crate::{Resolution, TirExpr, TirExprKind, TirFunction, TirStmt, TirUnOp};

pub const NODE_BUDGET: usize = 96;

pub fn absent_param(cond: &TirExpr) -> Option<u32> {
    let TirExprKind::Unary {
        op: TirUnOp::IsNull,
        operand,
    } = &cond.kind
    else {
        return None;
    };
    if let TirExpr {
        kind: TirExprKind::Var,
        res: Resolution::Param(i),
        ..
    } = operand.as_ref()
    {
        return Some(*i);
    }
    None
}

struct Census {
    nodes: usize,
    inlinable: bool,
}

impl Census {
    fn stmts(&mut self, stmts: &[TirStmt]) {
        for stmt in stmts {
            self.stmt(stmt);
        }
    }

    fn stmt(&mut self, stmt: &TirStmt) {
        self.nodes += 1;
        match stmt {
            TirStmt::Expr(e) | TirStmt::Throw(e) => self.expr(e),
            TirStmt::Let { init, .. } => init.iter().for_each(|e| self.expr(e)),
            TirStmt::Return(value) => value.iter().for_each(|e| self.expr(e)),
            TirStmt::If {
                cond,
                then_body,
                else_body,
            } => {
                self.inlinable &= absent_param(cond).is_none();
                self.expr(cond);
                self.stmts(then_body);
                self.stmts(else_body);
            }
            TirStmt::Loop { cond, body } => {
                self.expr(cond);
                self.stmts(body);
            }
            TirStmt::Break | TirStmt::Continue => {}
            TirStmt::Try { .. } | TirStmt::BuildClass(_) => self.inlinable = false,
        }
    }

    fn expr(&mut self, e: &TirExpr) {
        self.nodes += 1;
        match &e.kind {
            TirExprKind::Closure { .. }
            | TirExprKind::Await { .. }
            | TirExprKind::Yield { .. }
            | TirExprKind::SuperCall { .. }
            | TirExprKind::SuperMethodCall { .. } => self.inlinable = false,
            TirExprKind::Var => {
                self.inlinable &= !matches!(e.res, Resolution::Upvalue(_));
            }
            TirExprKind::Seq { stmts, .. } => self.stmts(stmts),
            TirExprKind::IntLit(_)
            | TirExprKind::FloatLit(_)
            | TirExprKind::BoolLit(_)
            | TirExprKind::StrLit(_)
            | TirExprKind::CharLit(_)
            | TirExprKind::NullLit
            | TirExprKind::Binary { .. }
            | TirExprKind::Unary { .. }
            | TirExprKind::Field { .. }
            | TirExprKind::Index { .. }
            | TirExprKind::Call { .. }
            | TirExprKind::MethodCall { .. }
            | TirExprKind::Assign { .. }
            | TirExprKind::ArrayLit(_)
            | TirExprKind::TupleLit(_)
            | TirExprKind::ObjectLit { .. }
            | TirExprKind::Discriminant { .. }
            | TirExprKind::VariantPayload { .. }
            | TirExprKind::TypeTest { .. }
            | TirExprKind::Cast { .. }
            | TirExprKind::Checked { .. }
            | TirExprKind::New { .. }
            | TirExprKind::MakeVariant { .. }
            | TirExprKind::Select { .. }
            | TirExprKind::RangeLit { .. }
            | TirExprKind::ObjectRest { .. }
            | TirExprKind::ExtensionCall { .. }
            | TirExprKind::ObjectKeys { .. }
            | TirExprKind::Length { .. }
            | TirExprKind::DecimalLit(_)
            | TirExprKind::BigIntLit(_) => {}
        }
        for child in child_exprs(e) {
            self.expr(child);
        }
    }
}

pub fn body_inlinable(tf: &TirFunction, force: bool) -> bool {
    if tf.is_async || tf.is_generator || tf.has_rest {
        return false;
    }
    let mut census = Census {
        nodes: 0,
        inlinable: true,
    };
    census.stmts(&tf.body);
    census.inlinable && (force || census.nodes <= NODE_BUDGET)
}
