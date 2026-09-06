//! What the checker proved about *which* entity an expression refers to.
//! Today the backend re-derives this at runtime: InvokeVirtual resolves a
//! method by name, globals are patched from name-keyed to index-keyed before
//! execution, and the method dispatcher strcmps against `push` and `pop`.
//! None of that is information the runtime has and the checker lacks.

use std::rc::Rc;
use varn_tir::{DynReason, Resolution};

/// A resolution either dispatches statically or it does not, and the
/// distinction is what the coverage report counts.
#[test]
fn static_dispatch_is_distinguishable() {
    assert!(Resolution::FieldSlot(3).is_static_dispatch());
    assert!(Resolution::VtableSlot(7).is_static_dispatch());
    assert!(Resolution::GlobalSlot(12).is_static_dispatch());
    assert!(!Resolution::ByName {
        name: Rc::from("x"),
        why: DynReason::IndexSignature,
    }
    .is_static_dispatch());
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
