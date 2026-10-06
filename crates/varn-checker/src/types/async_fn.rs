













use super::{CheckerTyTable, Type};
use varn_core::{AtomInterner, TypeKind};
















pub fn async_fn_return(
    ret: Type,
    is_async: bool,
    table: &mut CheckerTyTable,
    interner: &AtomInterner,
) -> Type {
    if !is_async || ret.is_dynamic() || is_awaitable(&ret, table, interner) {
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



pub fn is_awaitable(ty: &Type, table: &CheckerTyTable, interner: &AtomInterner) -> bool {
    match table.get(ty.0) {
        TypeKind::Generic(name, args, _) => {
            table.get_list(args).len() == 1
                && (interner.resolve(name) == varn_core::BuiltinType::Task.name()
                    || interner.resolve(name) == varn_core::BuiltinType::TaskHandle.name())
        }
        _ => false,
    }
}



pub fn awaited(ty: &Type, table: &CheckerTyTable, interner: &AtomInterner) -> Type {
    match table.get(ty.0) {
        TypeKind::Generic(_, args, _) if is_awaitable(ty, table, interner) => {
            Type::resolved(table.get_list(args)[0])
        }
        _ => *ty,
    }
}
