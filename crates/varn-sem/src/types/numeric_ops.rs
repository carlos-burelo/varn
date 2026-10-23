use super::numeric_literal::literal_operand_class;
use super::{CheckerTyTable, Type};
use varn_core::ast::{AstArena, ExprId};
use varn_core::TypeKind;

pub fn numeric_operand(t: &Type, table: &CheckerTyTable) -> Option<varn_core::NumericOperand> {
    use varn_core::NumericOperand;
    match table.get(t.0) {
        TypeKind::Primitive(varn_core::LangPrimitive::Int)
        | TypeKind::Literal(varn_core::TypeLiteral::Int(_)) => Some(NumericOperand::Int),
        TypeKind::Primitive(varn_core::LangPrimitive::Float) => Some(NumericOperand::Float),
        TypeKind::Primitive(varn_core::LangPrimitive::Decimal) => Some(NumericOperand::Decimal),
        TypeKind::Primitive(varn_core::LangPrimitive::BigInt) => Some(NumericOperand::BigInt),
        TypeKind::Primitive(_)
        | TypeKind::Literal(_)
        | TypeKind::This
        | TypeKind::Array(_)
        | TypeKind::Union(_)
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
        | TypeKind::TypePredicate { .. } => None,
    }
}

pub fn numeric_binary_type(l: &Type, r: &Type, table: &CheckerTyTable) -> Option<Type> {
    use varn_core::NumericOperand;
    let kind =
        varn_core::binary_operand_kind(numeric_operand(l, table), numeric_operand(r, table))?;
    Some(match kind {
        NumericOperand::Int => Type::Int,
        NumericOperand::Float => Type::Float,
        NumericOperand::Decimal => Type::Decimal,
        NumericOperand::BigInt => Type::BigInt,
    })
}

pub fn adopt_literal_operands(
    arena: &AstArena,
    left: ExprId,
    right: ExprId,
    l: Type,
    r: Type,
    table: &CheckerTyTable,
) -> (Type, Type) {
    let l2 = literal_operand_class(arena, left, &r, table).unwrap_or(l);
    let r2 = literal_operand_class(arena, right, &l, table).unwrap_or(r);
    (l2, r2)
}

pub fn numeric_operands_compatible(l: &Type, r: &Type, table: &CheckerTyTable) -> bool {
    numeric_binary_type(l, r, table).is_some()
}
