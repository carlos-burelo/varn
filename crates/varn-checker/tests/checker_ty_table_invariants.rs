#![allow(unused_crate_dependencies)]

use std::sync::Arc;
use varn_core::TypeKind;
use varn_sem::types::{CheckerTyId, CheckerTyTable, ObjectTypeMember, Type};

#[test]
fn intrinsic_ids_are_fixed_and_small() {
    let t = CheckerTyTable::new();
    assert_eq!(
        t.get(CheckerTyId::INT),
        TypeKind::Primitive(varn_core::LangPrimitive::Int)
    );
    assert_eq!(
        t.get(CheckerTyId::STR),
        TypeKind::Primitive(varn_core::LangPrimitive::Str)
    );
    assert_eq!(
        t.get(CheckerTyId::BOOL),
        TypeKind::Primitive(varn_core::LangPrimitive::Bool)
    );
    assert_eq!(
        t.get(CheckerTyId::FLOAT),
        TypeKind::Primitive(varn_core::LangPrimitive::Float)
    );
    assert_eq!(
        t.get(CheckerTyId::DYNAMIC),
        TypeKind::Primitive(varn_core::LangPrimitive::Dynamic)
    );
    assert_eq!(t.get(CheckerTyId::THIS), TypeKind::This);
}

#[test]
fn content_ids_are_order_independent() {
    let mut left = CheckerTyTable::new();
    let mut right = CheckerTyTable::new();

    let l_int = left.intern(TypeKind::Primitive(varn_core::LangPrimitive::Int));
    let l_arr = left.intern(TypeKind::Array(l_int));
    let l_list = left.intern_list(&[l_int, CheckerTyId::STR]);
    let l_union = left.intern(TypeKind::Union(l_list));

    let r_str = right.intern(TypeKind::Primitive(varn_core::LangPrimitive::Str));
    let r_int = right.intern(TypeKind::Primitive(varn_core::LangPrimitive::Int));
    let r_list = right.intern_list(&[r_int, r_str]);
    let r_union = right.intern(TypeKind::Union(r_list));
    let r_arr = right.intern(TypeKind::Array(r_int));

    assert_eq!(l_int, r_int);
    assert_eq!(l_arr, r_arr, "Array<int> es el mismo id en ambas tablas");
    assert_eq!(l_union, r_union, "Union<int,str> es el mismo id");
}

#[test]
fn intern_is_idempotent_and_get_roundtrips() {
    let mut t = CheckerTyTable::new();
    let a = t.intern(TypeKind::Array(CheckerTyId::INT));
    let b = t.intern(TypeKind::Array(CheckerTyId::STR));
    let a_again = t.intern(TypeKind::Array(CheckerTyId::INT));

    assert_eq!(a, a_again);
    assert_ne!(a, b);
    assert_eq!(t.get(a), TypeKind::Array(CheckerTyId::INT));
    assert_eq!(t.get(b), TypeKind::Array(CheckerTyId::STR));
}

#[test]
fn absorb_is_a_union_that_preserves_ids() {
    let mut local = CheckerTyTable::new();
    let local_arr = local.intern(TypeKind::Array(CheckerTyId::INT));

    let mut foreign = CheckerTyTable::new();
    let foreign_list = foreign.intern_list(&[CheckerTyId::INT, CheckerTyId::STR]);
    let foreign_union = foreign.intern(TypeKind::Union(foreign_list));
    let foreign_members = foreign.intern_object_members(vec![ObjectTypeMember::Property {
        name: Arc::from("ok"),
        ty: CheckerTyId::BOOL,
        optional: false,
        readonly: false,
    }]);
    let foreign_obj = foreign.intern(TypeKind::Object(foreign_members));

    local.absorb(&foreign);

    assert_eq!(local.get(foreign_union), foreign.get(foreign_union));
    assert_eq!(local.get(foreign_obj), foreign.get(foreign_obj));
    assert_eq!(local.get(local_arr), TypeKind::Array(CheckerTyId::INT));
}

#[test]
fn checker_ty_table_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<CheckerTyTable>();
    assert_send_sync::<Type>();
}

#[test]
fn bind_result_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<varn_sem::bind::BindResult>();
}

#[test]
fn ids_agree_across_threads_and_interning_order() {
    fn build(union_first: bool) -> (CheckerTyId, CheckerTyId, CheckerTyId) {
        let mut t = CheckerTyTable::new();
        if union_first {
            let l = t.intern_list(&[CheckerTyId::INT, CheckerTyId::STR]);
            let _u = t.intern(TypeKind::Union(l));
            let a = t.intern(TypeKind::Array(CheckerTyId::INT));
            let l2 = t.intern_list(&[CheckerTyId::INT, CheckerTyId::STR]);
            (
                a,
                t.intern(TypeKind::Union(l2)),
                t.intern(TypeKind::Array(a)),
            )
        } else {
            let a = t.intern(TypeKind::Array(CheckerTyId::INT));
            let l = t.intern_list(&[CheckerTyId::INT, CheckerTyId::STR]);
            let u = t.intern(TypeKind::Union(l));
            let a2 = t.intern(TypeKind::Array(CheckerTyId::INT));
            (a2, u, t.intern(TypeKind::Array(a)))
        }
    }

    let (left, right) = std::thread::scope(|s| {
        let h1 = s.spawn(|| build(true));
        let h2 = s.spawn(|| build(false));
        (h1.join().unwrap(), h2.join().unwrap())
    });
    assert_eq!(left, right, "mismos ids en hilos y órdenes distintos");
}
