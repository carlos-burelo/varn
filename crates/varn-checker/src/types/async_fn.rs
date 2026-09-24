//! The one place that knows what an `async` function's type is.
//!
//! Varn has no explicit `Promise` wrapper in source: writing `async` is what
//! makes the call produce a task, so `async f(): int` declares a body that
//! returns `int` and a *value* of type `Task<int>`. Somebody has to reconcile
//! those two readings, and if more than one place does it they disagree —
//! which is exactly what happened: the rule lived in five separate spots and
//! each covered a different subset of callables, so `await` on an imported
//! method, a namespace function or an async arrow warned about a "non-Future
//! type" while the same shape declared locally was fine.
//!
//! So the wrap happens once, where the function's type is built, and every
//! consumer downstream just reads the type.

use super::{CheckerTyTable, Type};
use crate::module_resolver::ImportResolver;
use varn_core::{AtomInterner, TypeKind};

/// The return type an `async` function's *type* carries, given the return type
/// its *body* produces. Idempotent: a body already declared as `Task<T>` or
/// `TaskHandle<T>` is left alone, so `async f(): Task<int>` and
/// `async f(): int` describe the same value.
///
/// `dynamic` is left alone too — wrapping it would claim knowledge the checker
/// does not have, and `await` accepts it already.
///
/// `resolver` mints the `Task` `Atom` into the SAME shared, per-compilation
/// `AtomInterner` every other `Generic` name comes from
/// (`ImportResolver::intern`) — required so a `Task<int>` built here compares
/// equal, by `Atom`, to a user-written `Task<int>` annotation resolved
/// through `resolve_type_node`. `None` (no resolver reachable) degrades to
/// leaving `ret` unwrapped rather than minting an `Atom` nothing else can
/// compare against.
pub fn async_fn_return(
    ret: Type,
    is_async: bool,
    table: &mut CheckerTyTable,
    interner: &AtomInterner,
    resolver: Option<&dyn ImportResolver>,
) -> Type {
    if !is_async || ret.is_dynamic() || is_awaitable(&ret, table, interner) {
        return ret;
    }
    match resolver {
        Some(r) => {
            let atom = r.intern(varn_core::BuiltinType::Task.name());
            Type::generic_atom(atom, vec![ret], None, table)
        }
        None => ret,
    }
}

/// The type a generator function's *value* has, given the type its `yield`s
/// produce. `async function*` produces an `AsyncGenerator<T>` — same shape as
/// `Generator<T>`, since the driver settles the body's awaits inside `next()`,
/// but a distinct name so `for await` and the `await` inside the body are
/// meaningful in the type system.
pub fn generator_of(
    yielded: Type,
    is_async: bool,
    table: &mut CheckerTyTable,
    resolver: Option<&dyn ImportResolver>,
) -> Type {
    let name = if is_async {
        "AsyncGenerator"
    } else {
        varn_core::BuiltinType::Generator.name()
    };
    match resolver {
        Some(r) => {
            let atom = r.intern(name);
            Type::generic_atom(atom, vec![yielded], None, table)
        }
        None => Type::Dynamic,
    }
}

/// Whether `await` on this type has something to unwrap. `Task` is what async
/// functions produce; `TaskHandle` is what `spawn` produces.
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

/// The value `await` yields: the payload of an awaitable, or the type itself
/// when there is nothing to unwrap.
pub fn awaited(ty: &Type, table: &CheckerTyTable, interner: &AtomInterner) -> Type {
    match table.get(ty.0) {
        TypeKind::Generic(_, args, _) if is_awaitable(ty, table, interner) => {
            Type(table.get_list(args)[0], false)
        }
        _ => *ty,
    }
}
