use crate::types::{CheckerTyId, CheckerTyTable, Type};
use varn_core::TypeKind;

#[inline]
pub(super) fn t(id: CheckerTyId) -> Type {
    Type::resolved(id)
}

pub(super) fn is_simple_type(ty: &Type, table: &CheckerTyTable) -> bool {
    matches!(
        table.get(ty.0),
        TypeKind::Primitive(_) | TypeKind::Builtin(_)
    )
}

pub(super) fn simple_types_compatible(
    declared: &Type,
    inferred: &Type,
    table: &CheckerTyTable,
) -> bool {
    use varn_core::LangPrimitive as P;
    match (table.get(declared.0), table.get(inferred.0)) {
        (TypeKind::Primitive(P::Dynamic), _) | (_, TypeKind::Primitive(P::Dynamic)) => true,
        (a, b) if a == b => true,
        (_, TypeKind::Primitive(P::Never)) => true,
        (TypeKind::Primitive(P::Decimal | P::BigInt), TypeKind::Primitive(P::Int)) => true,
        _ => false,
    }
}

use crate::types::numeric_literal::const_int_value;

pub(super) fn plain_literal_matches(
    target: &Type,
    arena: &varn_core::ast::AstArena,
    expr: varn_core::ast::ExprId,
    table: &CheckerTyTable,
) -> bool {
    use varn_core::ast::ExprKind;
    use varn_core::LangPrimitive as P;
    let TypeKind::Primitive(p) = table.get(target.0) else {
        return false;
    };
    matches!(
        (&arena.expr(expr).kind, p),
        (ExprKind::StrLiteral { .. }, P::Str)
            | (ExprKind::BoolLiteral { .. }, P::Bool)
            | (ExprKind::CharLiteral { .. }, P::Char)
            | (ExprKind::IntLiteral { .. }, P::Int)
            | (ExprKind::FloatLiteral { .. }, P::Float)
    )
}

pub(super) fn literal_expr_admitted(
    target: &Type,
    arena: &varn_core::ast::AstArena,
    expr: varn_core::ast::ExprId,
    table: &CheckerTyTable,
    interner: Option<&varn_core::AtomInterner>,
) -> bool {
    use varn_core::ast::ExprKind;
    use varn_core::TypeLiteral;
    let is_value = |lit: TypeLiteral<varn_core::Atom>| match (lit, &arena.expr(expr).kind) {
        (TypeLiteral::Str(atom), ExprKind::StrLiteral { value }) => {
            interner.and_then(|i| i.try_resolve(atom)) == Some(value.as_str())
        }
        (TypeLiteral::Bool(b), ExprKind::BoolLiteral { value }) => b == *value,
        (TypeLiteral::Char(c), ExprKind::CharLiteral { value }) => c == *value,
        (TypeLiteral::Int(v), _) => const_int_value(arena, expr) == Some(v),
        _ => false,
    };
    match table.get(target.0) {
        TypeKind::Literal(l) => is_value(l),
        TypeKind::Union(list) => table.get_list(list).iter().any(|m| match table.get(*m) {
            TypeKind::Literal(l) => is_value(l),
            TypeKind::Primitive(_) | TypeKind::Builtin(_) | TypeKind::This | TypeKind::Array(_) | TypeKind::Union(_) | TypeKind::Intersection(_) | TypeKind::Tuple(_) | TypeKind::Named(..) | TypeKind::Generic(..) | TypeKind::TemplateLiteral(_) | TypeKind::Fn(_) | TypeKind::Object(_) | TypeKind::Typeof(_) | TypeKind::KeyOf(_) | TypeKind::IndexedAccess { .. } | TypeKind::Mapped { .. } | TypeKind::Conditional { .. } | TypeKind::Infer(_) | TypeKind::EnumVariant { .. } | TypeKind::TypePredicate { .. } => false,
        }),
        TypeKind::Primitive(_) | TypeKind::Builtin(_) | TypeKind::This | TypeKind::Array(_) | TypeKind::Intersection(_) | TypeKind::Tuple(_) | TypeKind::Named(..) | TypeKind::Generic(..) | TypeKind::TemplateLiteral(_) | TypeKind::Fn(_) | TypeKind::Object(_) | TypeKind::Typeof(_) | TypeKind::KeyOf(_) | TypeKind::IndexedAccess { .. } | TypeKind::Mapped { .. } | TypeKind::Conditional { .. } | TypeKind::Infer(_) | TypeKind::EnumVariant { .. } | TypeKind::TypePredicate { .. } => false,
    }
}

pub(super) fn array_element_type(
    ty: &Type,
    table: &CheckerTyTable,
    interner: Option<&varn_core::AtomInterner>,
) -> Option<Type> {
    match table.get(ty.0) {
        TypeKind::Array(inner) => Some(Type::resolved(inner)),
        TypeKind::Generic(name, args, _)
            if table.get_list(args).len() == 1
                && interner
                    .is_some_and(|it| it.resolve(name) == varn_core::BuiltinType::Array.name()) =>
        {
            Some(Type::resolved(table.get_list(args)[0]))
        }
        TypeKind::Primitive(_) | TypeKind::Builtin(_) | TypeKind::Literal(_) | TypeKind::This | TypeKind::Union(_) | TypeKind::Intersection(_) | TypeKind::Tuple(_) | TypeKind::Named(..) | TypeKind::Generic(..) | TypeKind::TemplateLiteral(_) | TypeKind::Fn(_) | TypeKind::Object(_) | TypeKind::Typeof(_) | TypeKind::KeyOf(_) | TypeKind::IndexedAccess { .. } | TypeKind::Mapped { .. } | TypeKind::Conditional { .. } | TypeKind::Infer(_) | TypeKind::EnumVariant { .. } | TypeKind::TypePredicate { .. } => None,
    }
}
