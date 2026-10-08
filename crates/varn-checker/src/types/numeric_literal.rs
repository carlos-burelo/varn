use super::{CheckerTyTable, Type};
use varn_core::ast::{AstArena, ExprId, ExprKind, UnaryOp};
use varn_core::TypeKind;

const F64_EXACT_INT: i64 = 1 << 53;

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
        ExprKind::FloatLiteral { .. }
        | ExprKind::BigIntLiteral { .. }
        | ExprKind::DecimalLiteral { .. }
        | ExprKind::StrLiteral { .. }
        | ExprKind::CharLiteral { .. }
        | ExprKind::BoolLiteral { .. }
        | ExprKind::NullLiteral
        | ExprKind::RegexLiteral { .. }
        | ExprKind::Template { .. }
        | ExprKind::TaggedTemplate { .. }
        | ExprKind::Identifier { .. }
        | ExprKind::Missing
        | ExprKind::This
        | ExprKind::Super
        | ExprKind::Array { .. }
        | ExprKind::Object { .. }
        | ExprKind::Tuple { .. }
        | ExprKind::Record { .. }
        | ExprKind::Unary { .. }
        | ExprKind::Update { .. }
        | ExprKind::Binary { .. }
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
        | ExprKind::MetaAccess { .. } => None,
    }
}

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
            .any(|m| int_literal_adopts(&Type::resolved(*m), value, table)),
        TypeKind::Primitive(_)
        | TypeKind::Builtin(_)
        | TypeKind::Literal(_)
        | TypeKind::This
        | TypeKind::Array(_)
        | TypeKind::Intersection(_)
        | TypeKind::Tuple(_)
        | TypeKind::Named(..)
        | TypeKind::Generic(..)
        | TypeKind::TemplateLiteral(_)
        | TypeKind::Fn(_)
        | TypeKind::Object(_)
        | TypeKind::Typeof(_)
        | TypeKind::KeyOf(_)
        | TypeKind::IndexedAccess { .. }
        | TypeKind::Mapped { .. }
        | TypeKind::Conditional { .. }
        | TypeKind::Infer(_)
        | TypeKind::EnumVariant { .. }
        | TypeKind::TypePredicate { .. } => false,
    }
}

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
