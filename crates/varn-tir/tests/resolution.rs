//! What the checker proved about *which* entity an expression refers to.
//! Today the backend re-derives this at runtime: InvokeVirtual resolves a
//! method by name, globals are patched from name-keyed to index-keyed before
//! execution, and the method dispatcher strcmps against `push` and `pop`.
//! None of that is information the runtime has and the checker lacks.

use std::rc::Rc;
use varn_tir::{DynReason, FnId, LocalId, Resolution};

/// A resolution is in exactly one of three states: it resolves to nothing, it
/// resolves to a known entity, or it defers to a runtime name lookup. The
/// coverage report counts the last two and must not count the first.
#[test]
fn the_three_dispatch_states_are_distinguishable() {
    let by_name = Resolution::ByName {
        name: Rc::from("x"),
        why: DynReason::IndexSignature,
    };

    // Resolves to a known entity.
    for r in [
        Resolution::FieldSlot(3),
        Resolution::StaticField(1),
        Resolution::VtableSlot(7),
        Resolution::GlobalSlot(12),
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

    // Resolves to nothing: a literal carries this, and it is neither.
    assert!(!Resolution::None.is_static_dispatch());
    assert!(!Resolution::None.is_dynamic_dispatch());

    // Deferred to a runtime name lookup.
    assert!(!by_name.is_static_dispatch());
    assert!(by_name.is_dynamic_dispatch());
}

/// A by-name resolution always says why, so 1037 name-keyed reads can be
/// split into the ones that are honest and the ones that are bugs.
#[test]
fn by_name_carries_its_reason() {
    let r = Resolution::ByName {
        name: Rc::from("length"),
        why: DynReason::HostBoundary,
    };
    assert_eq!(r.dyn_reason(), Some(DynReason::HostBoundary));
    assert_eq!(Resolution::FieldSlot(0).dyn_reason(), None);
}
