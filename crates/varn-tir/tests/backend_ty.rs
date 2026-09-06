//! `BackendTy` is the single type the backend speaks. Two properties matter
//! more than its shape: it is `Copy`, because it is a field of every node; and
//! it has no `Default`, because "the type you get when you wrote none" is
//! exactly the hole this IR exists to close.

use varn_tir::{BackendTy, DynReason, TyTable};

/// Interning is stable: the same type interns to the same id, and reading it
/// back gives the same type.
#[test]
fn interning_round_trips_and_dedups() {
    let mut t = TyTable::default();
    let a = t.intern(BackendTy::Int);
    let b = t.intern(BackendTy::Int);
    assert_eq!(a, b, "the same type must intern to the same id");
    assert_eq!(t.get(a), BackendTy::Int);

    let arr_int = BackendTy::Array(a);
    let x = t.intern(arr_int);
    assert_eq!(t.get(x), arr_int);
    assert_ne!(x, a, "int and int[] are different types");
}

/// A list of types round-trips, for tuples and signatures.
#[test]
fn type_lists_round_trip() {
    let mut t = TyTable::default();
    let l = t.intern_list(&[BackendTy::Int, BackendTy::Str, BackendTy::Bool]);
    assert_eq!(
        t.get_list(l),
        &[BackendTy::Int, BackendTy::Str, BackendTy::Bool]
    );
}

/// Nullable keeps its payload instead of collapsing. `int?` used to reach the
/// backend as Dynamic, which is why a nullable scalar could never be a
/// (value, bit) pair. `non_nullable` is what every consumer calls to strip the
/// nullability before comparing types, so it is the thing worth testing.
#[test]
fn nullable_keeps_its_payload() {
    let mut t = TyTable::default();
    let int_id = t.intern(BackendTy::Int);
    let n = BackendTy::Nullable(int_id);

    // The payload survives, and reading it back is the declared type.
    assert_eq!(n.non_nullable(&t), BackendTy::Int);

    // Nesting collapses all the way down, not one level.
    let n_id = t.intern(n);
    let nn = BackendTy::Nullable(n_id);
    assert_eq!(nn.non_nullable(&t), BackendTy::Int);

    // A non-nullable type is its own payload.
    assert_eq!(BackendTy::Str.non_nullable(&t), BackendTy::Str);

    // Stripping nullability off a structured type keeps the structure.
    let arr = BackendTy::Array(int_id);
    let arr_id = t.intern(arr);
    assert_eq!(BackendTy::Nullable(arr_id).non_nullable(&t), arr);
}

/// Dynamic always says why. A count of dynamics is not actionable; a count per
/// reason is.
#[test]
fn dynamic_carries_its_reason() {
    let d = BackendTy::Dynamic(DynReason::HostBoundary);
    assert_ne!(d, BackendTy::Dynamic(DynReason::Unannotated));
}

/// `BackendTy` is Copy, so it can be a field of every node without cloning.
#[test]
fn backend_ty_is_copy() {
    fn assert_copy<T: Copy>() {}
    assert_copy::<BackendTy>();
    assert_copy::<DynReason>();
}
