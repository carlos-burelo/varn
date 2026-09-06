//! One authority for how a typed field is laid out.
//!
//! Today there are four sites computing this and two answers:
//! ClassLayout::from_fields discards field_repr() and uses 16 bytes for
//! everything, the checker's annotator packs by real alignment, the JIT
//! recomputes slot*16, and set_property looks the field up by name. The
//! checker's packed offset has no consumer, which is the only reason the
//! divergence is currently harmless.

use varn_tir::{BackendTy, ClassId, ClassInfo};
use std::rc::Rc;

/// A subclass's fields come after its parent's, so a pointer to the derived
/// class is a valid pointer to the base without adjustment.
#[test]
fn inheritance_lays_out_by_prefix() {
    let base = ClassInfo::new(Rc::from("Base"), None, vec![
        ("a".into(), BackendTy::Int),
        ("b".into(), BackendTy::Bool),
    ]);
    let derived = ClassInfo::new(Rc::from("Derived"), Some((ClassId(0), &base)), vec![
        ("c".into(), BackendTy::Float),
    ]);

    assert_eq!(derived.field("a").map(|f| f.slot), Some(0));
    assert_eq!(derived.field("b").map(|f| f.slot), Some(1));
    assert_eq!(derived.field("c").map(|f| f.slot), Some(2));
    assert_eq!(base.field("c").map(|f| f.slot), None);
    assert_eq!(derived.parent, Some(ClassId(0)), "the parent id is recorded");
    assert_eq!(base.parent, None);
}

/// Slots are dense and in declaration order within a class.
#[test]
fn slots_are_dense_and_ordered() {
    let c = ClassInfo::new(Rc::from("P"), None, vec![
        ("x".into(), BackendTy::Int),
        ("y".into(), BackendTy::Int),
        ("z".into(), BackendTy::Str),
    ]);
    let slots: Vec<u16> = c.fields.iter().map(|f| f.slot).collect();
    assert_eq!(slots, vec![0, 1, 2]);
}

/// A field's declared type reaches the layout. A layout reporting Dynamic for
/// a declared int means the type was dropped on the way in.
#[test]
fn declared_types_reach_the_layout() {
    let c = ClassInfo::new(Rc::from("P"), None, vec![
        ("n".into(), BackendTy::Int),
        ("s".into(), BackendTy::Str),
    ]);
    assert_eq!(c.field("n").map(|f| f.ty), Some(BackendTy::Int));
    assert_eq!(c.field("s").map(|f| f.ty), Some(BackendTy::Str));
}

/// Overriding a method reuses the parent's vtable index, which is what makes
/// the index a valid dispatch target for a base-typed receiver.
#[test]
fn override_reuses_the_parent_slot() {
    let base = ClassInfo::new_with_methods(
        Rc::from("Animal"), None, vec![],
        vec!["speak".into(), "name".into()],
    );
    let derived = ClassInfo::new_with_methods(
        Rc::from("Dog"), Some((ClassId(0), &base)), vec![],
        vec!["speak".into(), "fetch".into()],
    );

    assert_eq!(base.method_slot("speak"), Some(0));
    assert_eq!(derived.method_slot("speak"), Some(0), "override reuses the slot");
    assert_eq!(derived.method_slot("name"), Some(1), "inherited keeps its slot");
    assert_eq!(derived.method_slot("fetch"), Some(2), "new method appends");
}
