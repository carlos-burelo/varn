use super::{CheckerTyTable, Type};
use varn_core::TypeKind;

pub fn async_fn_return(ret: Type, is_async: bool, table: &mut CheckerTyTable) -> Type {
    if !is_async || ret.is_dynamic() || is_awaitable(&ret, table) {
        return ret;
    }
    let atom = table.intern_name(varn_core::BuiltinType::Task.name());
    Type::generic_atom(atom, vec![ret], None, table)
}

pub fn generator_of(yielded: Type, is_async: bool, table: &mut CheckerTyTable) -> Type {
    let name = if is_async {
        "AsyncGenerator"
    } else {
        varn_core::BuiltinType::Generator.name()
    };
    let atom = table.intern_name(name);
    Type::generic_atom(atom, vec![yielded], None, table)
}

pub fn is_awaitable(ty: &Type, table: &CheckerTyTable) -> bool {
    match table.get(ty.0) {
        TypeKind::Generic(name, args, _) => {
            table.get_list(args).len() == 1
                && matches!(
                    table.name(name),
                    Some(n)
                        if n == varn_core::BuiltinType::Task.name()
                            || n == varn_core::BuiltinType::TaskHandle.name()
                )
        }
        TypeKind::Primitive(_)
        | TypeKind::Builtin(_)
        | TypeKind::Literal(_)
        | TypeKind::This
        | TypeKind::Array(_)
        | TypeKind::Union(_)
        | TypeKind::Intersection(_)
        | TypeKind::Tuple(_)
        | TypeKind::Named(..)
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

pub fn awaited(ty: &Type, table: &CheckerTyTable) -> Type {
    match table.get(ty.0) {
        TypeKind::Generic(_, args, _) if is_awaitable(ty, table) => {
            Type::resolved(table.get_list(args)[0])
        }
        TypeKind::Primitive(_)
        | TypeKind::Builtin(_)
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
        | TypeKind::TypePredicate { .. } => *ty,
    }
}
