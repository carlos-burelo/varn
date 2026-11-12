use super::CallTypeCtx;
use vn_core::ast::ExprId;
use vn_core::TypeKind;
use vn_sem::types::Type;

pub(super) fn infer_await(c: &mut CallTypeCtx, argument: ExprId) -> Option<Type> {
    let ty = c.infer(argument)?;
    match c.table.get(ty.0) {
        TypeKind::Generic(_, args, _)
            if vn_sem::types::is_awaitable(&ty, c.table) && c.table.get_list(args).len() == 1 =>
        {
            Some(Type::resolved(c.table.get_list(args)[0]))
        }
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
        | TypeKind::Record(_)
        | TypeKind::Interface(_)
        | TypeKind::Typeof(_)
        | TypeKind::KeyOf(_)
        | TypeKind::IndexedAccess { .. }
        | TypeKind::Mapped { .. }
        | TypeKind::Conditional { .. }
        | TypeKind::Infer(_)
        | TypeKind::EnumVariant { .. }
        | TypeKind::TypePredicate { .. } => Some(ty),
    }
}

pub(super) fn infer_conditional(
    c: &mut CallTypeCtx,
    consequent: ExprId,
    alternate: ExprId,
) -> Option<Type> {
    let t = c.infer(consequent)?;
    let f = c.infer(alternate)?;
    if t == f {
        Some(t)
    } else {
        Some(Type::union(vec![t, f], c.table))
    }
}
