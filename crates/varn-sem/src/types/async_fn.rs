use super::{CheckerTyTable, Type};
use varn_core::{BuiltinType, TypeKind};

pub fn async_fn_return(ret: Type, is_async: bool, table: &mut CheckerTyTable) -> Type {
    if !is_async || ret.is_dynamic() || is_awaitable(&ret, table) {
        return ret;
    }
    Type::builtin(BuiltinType::Task, vec![ret], table)
}

pub fn callable_return(
    declared: Option<Type>,
    is_async: bool,
    is_generator: bool,
    table: &mut CheckerTyTable,
) -> Type {
    if is_generator {
        declared_generator(declared.unwrap_or(Type::Dynamic), is_async, table)
    } else {
        async_fn_return(declared.unwrap_or(Type::Void), is_async, table)
    }
}

pub fn yielded_by(declared: Type, table: &CheckerTyTable) -> Type {
    match declared.builtin_of(table) {
        Some(BuiltinType::Generator | BuiltinType::AsyncGenerator) => {
            declared.builtin_arg(0, table)
        }
        Some(_) | None => declared,
    }
}

pub fn declared_generator(declared: Type, is_async: bool, table: &mut CheckerTyTable) -> Type {
    let yielded = yielded_by(declared, table);
    generator_of(yielded, is_async, table)
}

pub fn generator_of(yielded: Type, is_async: bool, table: &mut CheckerTyTable) -> Type {
    let generator = if is_async {
        BuiltinType::AsyncGenerator
    } else {
        BuiltinType::Generator
    };
    Type::builtin(generator, vec![yielded], table)
}

pub fn is_awaitable(ty: &Type, table: &CheckerTyTable) -> bool {
    matches!(table.get(ty.0), TypeKind::Generic(_, args, _) if table.get_list(args).len() == 1)
        && matches!(
            ty.builtin_of(table),
            Some(BuiltinType::Task | BuiltinType::TaskHandle)
        )
}

pub fn awaited(ty: &Type, table: &CheckerTyTable) -> Type {
    match table.get(ty.0) {
        TypeKind::Generic(_, args, _) if is_awaitable(ty, table) => {
            Type::resolved(table.get_list(args)[0])
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
