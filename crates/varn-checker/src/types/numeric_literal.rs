//! An exact integer literal adopts the numeric type of its context (spec §5).
//! It is the only implicit conversion into `float`; variables never convert.

use super::{CheckerTyTable, Type};
use varn_core::ast::{AstArena, ExprId, ExprKind, UnaryOp};
use varn_core::TypeKind;

/// Largest magnitude every integer up to which `f64` represents exactly.
const F64_EXACT_INT: i64 = 1 << 53;

/// The value of an integer literal, sign and parentheses folded.
pub(crate) fn const_int_value(arena: &AstArena, expr: ExprId) -> Option<i64> {
    match &arena.expr(expr).kind {
        ExprKind::IntLiteral { value, .. } => Some(*value),
        ExprKind::Paren { expression } => const_int_value(arena, *expression),
        ExprKind::Unary {
            op: UnaryOp::Minus,
            prefix: true,
            operand,
            ..
        } => const_int_value(arena, *operand).and_then(i64::checked_neg),
        ExprKind::Unary {
            op: UnaryOp::Plus,
            prefix: true,
            operand,
            ..
        } => const_int_value(arena, *operand),
        _ => None,
    }
}

/// Whether the integer `value` can take `target`'s numeric type exactly.
pub(crate) fn int_literal_adopts(target: &Type, value: i64, table: &CheckerTyTable) -> bool {
    match table.get(target.0) {
        TypeKind::Primitive(varn_core::LangPrimitive::Float) => {
            (-F64_EXACT_INT..=F64_EXACT_INT).contains(&value)
        }
        TypeKind::Primitive(
            varn_core::LangPrimitive::Decimal | varn_core::LangPrimitive::BigInt,
        ) => true,
        TypeKind::Union(list) => table
            .get_list(list)
            .iter()
            .any(|m| int_literal_adopts(&Type(*m, false), value, table)),
        _ => false,
    }
}

/// `other` when `expr` is an integer literal that can take `other`'s numeric
/// type (`f * 2`, `d + 1`).
pub(crate) fn literal_operand_class(
    arena: &AstArena,
    expr: ExprId,
    other: &Type,
    table: &CheckerTyTable,
) -> Option<Type> {
    let value = const_int_value(arena, expr)?;
    int_literal_adopts(other, value, table).then_some(*other)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_adopts_only_exact_integers() {
        let table = CheckerTyTable::new();
        assert!(int_literal_adopts(&Type::Float, 1 << 53, &table));
        assert!(!int_literal_adopts(&Type::Float, (1 << 53) + 1, &table));
        assert!(int_literal_adopts(&Type::Decimal, i64::MAX, &table));
        assert!(int_literal_adopts(&Type::BigInt, i64::MIN, &table));
        assert!(!int_literal_adopts(&Type::Str, 1, &table));
        assert!(!int_literal_adopts(&Type::Int, 1, &table));
    }
}
