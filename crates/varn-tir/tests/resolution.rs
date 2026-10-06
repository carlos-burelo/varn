#![allow(unused_crate_dependencies)]

use std::sync::Arc;
use varn_tir::{DynReason, FnId, LocalId, Resolution};

#[test]
fn the_three_dispatch_states_are_distinguishable() {
    let by_name = Resolution::ByName {
        name: Arc::from("x"),
        why: DynReason::IndexSignature,
    };

    for r in [
        Resolution::FieldSlot(3),
        Resolution::StaticField(1),
        Resolution::VtableSlot(7),
        Resolution::GlobalSlot(12),
        Resolution::NativeGlobal(4),
        Resolution::Local(LocalId(0)),
        Resolution::Param(2),
        Resolution::Upvalue(1),
        Resolution::DirectFn(FnId(4)),
        Resolution::Intrinsic(9),
        Resolution::NativeOp(11),
    ] {
        assert!(r.is_static_dispatch(), "{r:?} should be static dispatch");
        assert!(!r.is_dynamic_dispatch(), "{r:?} is not by-name");
    }

    assert!(!Resolution::None.is_static_dispatch());
    assert!(!Resolution::None.is_dynamic_dispatch());

    assert!(!by_name.is_static_dispatch());
    assert!(by_name.is_dynamic_dispatch());
}

#[test]
fn by_name_carries_its_reason() {
    let r = Resolution::ByName {
        name: Arc::from("length"),
        why: DynReason::HostBoundary,
    };
    assert_eq!(r.dyn_reason(), Some(DynReason::HostBoundary));
    assert_eq!(Resolution::FieldSlot(0).dyn_reason(), None);
}
