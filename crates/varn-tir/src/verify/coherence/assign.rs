use crate::ty::BackendTy;
use crate::TirModule;

/// Whether a value of type `from` may be used where `to` is expected.
///
/// Not equality: `T` is assignable to `T?` — a non-null value is a valid
/// nullable — while `T?` to `T` is not, because that needs narrowing. Both
/// sides being Dynamic-tolerant keeps an honestly dynamic value from being
/// reported anywhere.
///
/// Uses a depth bound because coherence runs unconditionally even after
/// wellformed finds errors, so a cyclic type can reach here. Unlike check_ty,
/// this helper cannot rely on wellformed having run first.
pub(super) fn assignable(m: &TirModule, from: BackendTy, to: BackendTy) -> bool {
    assignable_with_depth(m, from, to, 0)
}

const ASSIGNABLE_DEPTH_LIMIT: usize = 32;

fn assignable_with_depth(m: &TirModule, from: BackendTy, to: BackendTy, depth: usize) -> bool {
    if depth > ASSIGNABLE_DEPTH_LIMIT {
        // Cycle detected or pathologically deep nesting. Return true so a
        // cyclic type doesn't become a false positive; the real error is in
        // wellformed if the cycle is wrong, not here.
        return true;
    }

    if matches!(from, BackendTy::Dynamic(_)) || matches!(to, BackendTy::Dynamic(_)) {
        return true;
    }
    if from == to {
        return true;
    }
    // Never inhabits every type: a call that always throws can stand in
    // anywhere.
    if from == BackendTy::Never {
        return true;
    }
    // `int` widens implicitly to the other numeric types — this is how a call
    // like `takesFloat(1)` or `takesDecimal(1)` type-checks in the language,
    // so a call argument is assignable across it. Arithmetic stays strict:
    // `check_binary` compares by equality, not through here.
    if from == BackendTy::Int
        && matches!(
            to,
            BackendTy::Float | BackendTy::Decimal | BackendTy::BigInt
        )
    {
        return true;
    }
    // Arrays and sets are covariant in their element for assignability — the
    // checker treats them so, and the backend representation is a pointer
    // either way.
    match (from, to) {
        (BackendTy::Array(a), BackendTy::Array(b)) | (BackendTy::Set(a), BackendTy::Set(b))
            if m.types.contains(a) && m.types.contains(b) =>
        {
            return assignable_with_depth(m, m.types.get(a), m.types.get(b), depth + 1);
        }
        _ => {}
    }
    // A subclass is assignable to any of its ancestors.
    if let (BackendTy::Class(sub), BackendTy::Class(sup)) = (from, to) {
        let mut cur = Some(sub);
        let mut hops = 0;
        while let Some(c) = cur {
            if c == sup {
                return true;
            }
            if hops > ASSIGNABLE_DEPTH_LIMIT {
                break;
            }
            hops += 1;
            cur = m.class(c).and_then(|ci| ci.parent);
        }
    }
    // The bare null value — `Nullable` over a `Never` payload — is assignable
    // to every nullable type. It is what `return null` in a `T?` function
    // produces.
    if let (BackendTy::Nullable(fi), BackendTy::Nullable(_)) = (from, to) {
        if m.types.contains(fi) && m.types.get(fi) == BackendTy::Never {
            return true;
        }
    }
    // T is assignable to T?; the reverse is not.
    if let BackendTy::Nullable(inner) = to {
        // A dangling handle means wellformed already reported the real
        // problem elsewhere; return true (the same safe direction as the
        // depth bound above) rather than indexing blindly.
        if !m.types.contains(inner) {
            return true;
        }
        return assignable_with_depth(m, from, m.types.get(inner), depth + 1);
    }
    false
}
