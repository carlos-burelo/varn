use super::super::name_suggestions::closest_in_list;
use super::Checker;
use std::sync::Arc;
use varn_core::ast::operators::BinaryOp;
use varn_core::ast::{ExprId, ExprKind};
use varn_core::{Diagnostic, ErrorCode};

impl<'r> Checker<'r> {
    pub(super) fn report_int_overflow(&mut self, expr: ExprId) {
        let range = self.ast_arena.expr(expr).range;
        self.diagnostics.push(
            Diagnostic::error(
                ErrorCode::IntegerOverflow,
                format!(
                    "this expression overflows int ({}..={})",
                    varn_core::INT_MIN,
                    varn_core::INT_MAX
                ),
            )
            .with_file(self.source_file.clone())
            .with_range(range),
        );
    }
}

pub(super) fn closest_name(
    name: &str,
    scope: &crate::scope::CheckerScope,
    arena: &crate::scope::ScopeArena,
    interner: &varn_core::AtomInterner,
) -> Option<String> {
    let mut all: Vec<String> = Vec::new();
    let mut current = scope;
    loop {
        all.extend(
            current
                .bindings
                .keys()
                .map(|k| interner.resolve(*k).to_string()),
        );
        match current.parent {
            Some(parent_id) => current = arena.get(parent_id),
            None => break,
        }
    }
    let all_rc: Vec<Arc<str>> = all.into_iter().map(Arc::from).collect();
    closest_in_list(name, &all_rc).map(|s| s.to_owned())
}

#[derive(PartialEq)]
pub(super) enum ConstInt {
    Value(i64),
    Overflow,
    NotConst,
}

pub(super) fn const_int_expr(e: ExprId, arena: &varn_core::ast::AstArena) -> ConstInt {
    use ConstInt::*;
    let lift = |o: Option<i64>| o.map_or(Overflow, Value);
    match &arena.expr(e).kind {
        ExprKind::IntLiteral { value, .. } => Value(*value),
        ExprKind::Unary { op, operand, .. } => {
            use varn_core::ast::operators::UnaryOp;
            let v = match const_int_expr(*operand, arena) {
                Value(v) => v,
                other => return other,
            };
            match op {
                UnaryOp::Minus => lift(varn_core::neg_int(v)),
                UnaryOp::Plus => Value(v),
                _ => NotConst,
            }
        }
        ExprKind::Paren { expression } => const_int_expr(*expression, arena),
        ExprKind::Binary { left, right, op } => {
            let a = match const_int_expr(*left, arena) {
                Value(v) => v,
                other => return other,
            };
            let b = match const_int_expr(*right, arena) {
                Value(v) => v,
                other => return other,
            };
            match op {
                BinaryOp::Add => lift(varn_core::add_int(a, b)),
                BinaryOp::Sub => lift(varn_core::sub_int(a, b)),
                BinaryOp::Mul => lift(varn_core::mul_int(a, b)),
                BinaryOp::Pow => match u32::try_from(b) {
                    Ok(e) => lift(varn_core::pow_int(a, e)),
                    Err(_) => NotConst,
                },
                _ => NotConst,
            }
        }
        _ => NotConst,
    }
}

pub(super) fn overflows_int_literal(e: ExprId, arena: &varn_core::ast::AstArena) -> bool {
    const_int_expr(e, arena) == ConstInt::Overflow
}
