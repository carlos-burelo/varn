use super::{CheckerTyTable, Type};
use vn_core::ast::{AstArena, ExprId, ExprKind, UnaryOp};
use vn_core::TypeKind;

const F64_EXACT_INT: i64 = 1 << 53;

pub fn const_int_value(arena: &AstArena, expr: ExprId) -> Option<i64> {
    const_int_literal(arena, expr, &mut Vec::new())
}

pub fn const_int_literal(arena: &AstArena, expr: ExprId, nodes: &mut Vec<ExprId>) -> Option<i64> {
    nodes.push(expr);
    match &arena.expr(expr).kind {
        ExprKind::IntLiteral { value, .. } => Some(*value),
        ExprKind::Paren { expression } => const_int_literal(arena, *expression, nodes),
        ExprKind::Unary {
            op: UnaryOp::Minus,
            prefix: true,
            operand,
            ..
        } => const_int_literal(arena, *operand, nodes).and_then(i64::checked_neg),
        ExprKind::Unary {
            op: UnaryOp::Plus,
            prefix: true,
            operand,
            ..
        } => const_int_literal(arena, *operand, nodes),
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

pub fn int_literal_adopts(target: &Type, value: i64, table: &CheckerTyTable) -> bool {
    int_literal_adoption(target, value, table).is_some()
}

pub fn int_literal_adoption(target: &Type, value: i64, table: &CheckerTyTable) -> Option<Type> {
    use vn_core::LangPrimitive as P;
    match table.get(target.0) {
        TypeKind::Primitive(P::Float) => (-F64_EXACT_INT..=F64_EXACT_INT)
            .contains(&value)
            .then_some(Type::Float),
        TypeKind::Primitive(P::Decimal) => Some(Type::Decimal),
        TypeKind::Primitive(P::BigInt) => Some(Type::BigInt),
        TypeKind::Union(list) => {
            let members = table.get_list(list);
            let admits_int = members.iter().any(|m| {
                matches!(
                    table.get(*m),
                    TypeKind::Primitive(P::Int) | TypeKind::Literal(vn_core::TypeLiteral::Int(_))
                )
            });
            if admits_int {
                return None;
            }
            [Type::Float, Type::Decimal, Type::BigInt]
                .into_iter()
                .filter(|candidate| members.contains(&candidate.0))
                .find_map(|candidate| int_literal_adoption(&candidate, value, table))
        }
        TypeKind::Primitive(_)
        | TypeKind::Literal(_)
        | TypeKind::This
        | TypeKind::Array(_)
        | TypeKind::Intersection(_)
        | TypeKind::Tuple(_)
        | TypeKind::Named(..)
        | TypeKind::Generic(..)
        | TypeKind::TemplateLiteral(_)
        | TypeKind::Fn(_)
        | TypeKind::Record(_)
        | TypeKind::Interface(_)
        | TypeKind::Typeof(_)
        | TypeKind::KeyOf(_)
        | TypeKind::IndexedAccess { .. }
        | TypeKind::Mapped { .. }
        | TypeKind::Conditional { .. }
        | TypeKind::Infer(_)
        | TypeKind::EnumVariant { .. }
        | TypeKind::TypePredicate { .. } => None,
    }
}

pub fn literal_operand_class(
    arena: &AstArena,
    expr: ExprId,
    other: &Type,
    table: &CheckerTyTable,
) -> Option<Type> {
    let value = const_int_value(arena, expr)?;
    int_literal_adoption(other, value, table)
}
