#![allow(unused_crate_dependencies)]

use std::sync::Arc;
use varn_tir::{BackendTy, ClassId, ClassInfo, SigId};

#[test]
fn inheritance_lays_out_by_prefix() {
    let base = ClassInfo::new(
        Arc::from("Base"),
        varn_tir::Ancestry::Root,
        vec![("a".into(), BackendTy::Int), ("b".into(), BackendTy::Bool)],
        &varn_tir::TyTable::default(),
    );
    let derived = ClassInfo::new(
        Arc::from("Derived"),
        varn_tir::Ancestry::Local(ClassId(0), &base),
        vec![("c".into(), BackendTy::Float)],
        &varn_tir::TyTable::default(),
    );

    assert_eq!(derived.field("a").map(|f| f.slot), Some(0));
    assert_eq!(derived.field("b").map(|f| f.slot), Some(1));
    assert_eq!(derived.field("c").map(|f| f.slot), Some(2));
    assert_eq!(base.field("c").map(|f| f.slot), None);
    assert_eq!(
        derived.parent,
        Some(ClassId(0)),
        "the parent id is recorded"
    );
    assert_eq!(base.parent, None);
}

#[test]
fn slots_are_dense_and_ordered() {
    let c = ClassInfo::new(
        Arc::from("P"),
        varn_tir::Ancestry::Root,
        vec![
            ("x".into(), BackendTy::Int),
            ("y".into(), BackendTy::Int),
            ("z".into(), BackendTy::Str),
        ],
        &varn_tir::TyTable::default(),
    );
    let slots: Vec<u16> = c.fields.iter().map(|f| f.slot).collect();
    assert_eq!(slots, vec![0, 1, 2]);
}

#[test]
fn declared_types_reach_the_layout() {
    let c = ClassInfo::new(
        Arc::from("P"),
        varn_tir::Ancestry::Root,
        vec![("n".into(), BackendTy::Int), ("s".into(), BackendTy::Str)],
        &varn_tir::TyTable::default(),
    );
    assert_eq!(c.field("n").map(|f| f.ty), Some(BackendTy::Int));
    assert_eq!(c.field("s").map(|f| f.ty), Some(BackendTy::Str));
}

#[test]
fn override_reuses_the_parent_slot() {
    let base = ClassInfo::new_with_methods(
        Arc::from("Animal"),
        varn_tir::Ancestry::Root,
        vec![],
        vec![("speak".into(), SigId(0)), ("name".into(), SigId(1))],
        &varn_tir::TyTable::default(),
    );
    let derived = ClassInfo::new_with_methods(
        Arc::from("Dog"),
        varn_tir::Ancestry::Local(ClassId(0), &base),
        vec![],
        vec![("speak".into(), SigId(2)), ("fetch".into(), SigId(3))],
        &varn_tir::TyTable::default(),
    );

    assert_eq!(base.method_slot("speak"), Some(0));
    assert_eq!(
        derived.method_slot("speak"),
        Some(0),
        "override reuses the slot"
    );
    assert_eq!(
        derived.method_slot("name"),
        Some(1),
        "inherited keeps its slot"
    );
    assert_eq!(derived.method_slot("fetch"), Some(2), "new method appends");

    assert_eq!(
        derived.vtable[0].sig,
        SigId(2),
        "override updates the signature"
    );
    assert_eq!(
        derived.vtable[1].sig,
        SigId(1),
        "inherited keeps the signature"
    );
    assert_eq!(
        derived.vtable[2].sig,
        SigId(3),
        "new method has its signature"
    );
}
