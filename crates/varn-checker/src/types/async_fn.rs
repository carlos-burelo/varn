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

use super::Type;
use varn_core::{IntrinsicType, TypeKind};

/// The return type an `async` function's *type* carries, given the return type
/// its *body* produces. Idempotent: a body already declared as `Task<T>` or
/// `TaskHandle<T>` is left alone, so `async f(): Task<int>` and
/// `async f(): int` describe the same value.
///
/// `dynamic` is left alone too — wrapping it would claim knowledge the checker
/// does not have, and `await` accepts it already.
pub fn async_fn_return(ret: Type, is_async: bool) -> Type {
    if !is_async || ret.is_dynamic() || is_awaitable(&ret) {
        return ret;
    }
    Type::generic(IntrinsicType::Task.as_str().to_owned(), vec![ret])
}

/// Whether `await` on this type has something to unwrap. `Task` is what async
/// functions produce; `TaskHandle` is what `spawn` produces.
pub fn is_awaitable(ty: &Type) -> bool {
    matches!(
        &ty.0,
        TypeKind::Generic(name, args, _)
            if args.len() == 1
                && (name.as_ref() == IntrinsicType::Task.as_str()
                    || name.as_ref() == IntrinsicType::TaskHandle.as_str())
    )
}

/// The value `await` yields: the payload of an awaitable, or the type itself
/// when there is nothing to unwrap.
pub fn awaited(ty: &Type) -> Type {
    match &ty.0 {
        TypeKind::Generic(_, args, _) if is_awaitable(ty) => args[0].clone(),
        _ => ty.clone(),
    }
}
