use super::super::name_suggestions::closest_in_list;
use super::Checker;
use std::sync::Arc;
use vn_core::ast::operators::BinaryOp;
use vn_core::ast::{ExprId, ExprKind};
use vn_core::{Diagnostic, ErrorCode};

impl<'r> Checker<'r> {
    pub(super) fn report_int_overflow(&mut self, expr: ExprId) {
        let range = self.ast_arena.expr(expr).range;
        self.diagnostics.push(
            Diagnostic::error(
                ErrorCode::IntegerOverflow,
                format!(
                    "this expression overflows int ({}..={})",
                    vn_core::INT_MIN,
                    vn_core::INT_MAX
                ),
            )
            .with_file(self.source_file.clone())
            .with_range(range),
        );
    }
}

pub(super) fn closest_name(
    name: &str,
    scope: &vn_sem::scope::CheckerScope,
    arena: &vn_sem::scope::ScopeArena,
    interner: &vn_core::AtomInterner,
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

pub(super) fn const_int_expr(e: ExprId, arena: &vn_core::ast::AstArena) -> ConstInt {
    use ConstInt::*;
    let lift = |o: Option<i64>| o.map_or(Overflow, Value);
    match &arena.expr(e).kind {
        ExprKind::IntLiteral { value, .. } => Value(*value),
        ExprKind::Unary { op, operand, .. } => {
            use vn_core::ast::operators::UnaryOp;
            let v = match const_int_expr(*operand, arena) {
                Value(v) => v,
                other @ Overflow | other @ NotConst => return other,
            };
            match op {
                UnaryOp::Minus => lift(vn_core::neg_int(v)),
                UnaryOp::Plus => Value(v),
                UnaryOp::Not | UnaryOp::BitNot => NotConst,
            }
        }
        ExprKind::Paren { expression } => const_int_expr(*expression, arena),
        ExprKind::Binary { left, right, op } => {
            let a = match const_int_expr(*left, arena) {
                Value(v) => v,
                other @ Overflow | other @ NotConst => return other,
            };
            let b = match const_int_expr(*right, arena) {
                Value(v) => v,
                other @ Overflow | other @ NotConst => return other,
            };
            match op {
                BinaryOp::Add => lift(vn_core::add_int(a, b)),
                BinaryOp::Sub => lift(vn_core::sub_int(a, b)),
                BinaryOp::Mul => lift(vn_core::mul_int(a, b)),
                BinaryOp::Pow => match u64::try_from(b) {
                    Ok(e) => lift(vn_core::pow_int(a, e)),
                    Err(_) => NotConst,
                },
                BinaryOp::Div
                | BinaryOp::Mod
                | BinaryOp::Eq
                | BinaryOp::NotEq
                | BinaryOp::Lt
                | BinaryOp::Gt
                | BinaryOp::LtEq
                | BinaryOp::GtEq
                | BinaryOp::BitAnd
                | BinaryOp::BitOr
                | BinaryOp::BitXor
                | BinaryOp::Shl
                | BinaryOp::Shr
                | BinaryOp::UShr
                | BinaryOp::In => NotConst,
            }
        }
        ExprKind::FloatLiteral { .. }
        | ExprKind::BigIntLiteral { .. }
        | ExprKind::DecimalLiteral { .. }
        | ExprKind::StrLiteral { .. }
        | ExprKind::CharLiteral { .. }
        | ExprKind::BoolLiteral { .. }
        | ExprKind::NullLiteral
        | ExprKind::Template { .. }
        | ExprKind::TaggedTemplate { .. }
        | ExprKind::Identifier { .. }
        | ExprKind::Missing
        | ExprKind::This
        | ExprKind::ImportMeta(_)
        | ExprKind::Super
        | ExprKind::Array { .. }
        | ExprKind::Object { .. }
        | ExprKind::Tuple { .. }
        | ExprKind::Update { .. }
        | ExprKind::Logical { .. }
        | ExprKind::Assign { .. }
        | ExprKind::Conditional { .. }
        | ExprKind::Member { .. }
        | ExprKind::Call { .. }
        | ExprKind::New { .. }
        | ExprKind::Function { .. }
        | ExprKind::Arrow { .. }
        | ExprKind::Sequence { .. }
        | ExprKind::Await { .. }
        | ExprKind::Spawn { .. }
        | ExprKind::Yield { .. }
        | ExprKind::Spread { .. }
        | ExprKind::Pipeline { .. }
        | ExprKind::Range { .. }
        | ExprKind::NonNull { .. }
        | ExprKind::Try { .. }
        | ExprKind::As { .. }
        | ExprKind::Satisfies { .. }
        | ExprKind::ClassExpr { .. }
        | ExprKind::Match { .. }
        | ExprKind::Is { .. }
        | ExprKind::With { .. }
        | ExprKind::MetaAccess { .. } => NotConst,
    }
}

pub(super) fn overflows_int_literal(e: ExprId, arena: &vn_core::ast::AstArena) -> bool {
    const_int_expr(e, arena) == ConstInt::Overflow
}
