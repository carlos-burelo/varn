#![allow(unused_crate_dependencies)]

use varn_tir::{BackendTy, DynReason, TyTable};

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

#[test]
fn type_lists_round_trip() {
    let mut t = TyTable::default();
    let l = t.intern_list(&[BackendTy::Int, BackendTy::Str, BackendTy::Bool]);
    assert_eq!(
        t.get_list(l),
        &[BackendTy::Int, BackendTy::Str, BackendTy::Bool]
    );
}

#[test]
fn nullable_keeps_its_payload() {
    let mut t = TyTable::default();
    let int_id = t.intern(BackendTy::Int);
    let n = BackendTy::Nullable(int_id);

    assert_eq!(n.non_nullable(&t), BackendTy::Int);

    let n_id = t.intern(n);
    let nn = BackendTy::Nullable(n_id);
    assert_eq!(nn.non_nullable(&t), BackendTy::Int);

    assert_eq!(BackendTy::Str.non_nullable(&t), BackendTy::Str);

    let arr = BackendTy::Array(int_id);
    let arr_id = t.intern(arr);
    assert_eq!(BackendTy::Nullable(arr_id).non_nullable(&t), arr);
}

#[test]
fn dynamic_carries_its_reason() {
    let d = BackendTy::Dynamic(DynReason::HostBoundary);
    assert_ne!(d, BackendTy::Dynamic(DynReason::Declared));
}

#[test]
fn backend_ty_is_copy() {
    fn assert_copy<T: Copy>() {}
    assert_copy::<BackendTy>();
    assert_copy::<DynReason>();
}
