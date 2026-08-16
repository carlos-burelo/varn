//! Modifier combinations the language does not admit.
//!
//! These are grammar-level rules — they need no types to decide — so they are
//! settled here rather than in the checker, once, for every form that can
//! carry the modifiers.

/// `async function*` is not a thing in Varn.
///
/// The parser used to accept it and everything downstream then ignored the
/// `async`: the value typed as `Generator<T>`, `next()` returned the item
/// object rather than a task, and an `await` inside the body corrupted the
/// resumed frame — `value is not callable: 0.0000…64 (type: float)`. Accepting
/// a modifier that nothing honours is worse than not having it.
pub(crate) fn reject_async_generator(is_async: bool, is_generator: bool) -> Result<(), String> {
    if is_async && is_generator {
        return Err(String::from(
            "`async` generators are not supported — a generator is driven synchronously, \
             so `await` inside one has nothing to suspend into",
        ));
    }
    Ok(())
}
