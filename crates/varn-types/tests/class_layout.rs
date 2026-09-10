use std::rc::Rc;
use varn_core::TypeTag;
use varn_types::value::ClassObj;

/// The declared type of a field reaches the layout the runtime allocates by.
/// It arrives with the declaration — nothing downstream can re-derive it — so
/// a layout reporting `Dynamic` for a declared `int` means the operand carrying
/// it was dropped somewhere between the compiler and here.
#[test]
fn declared_field_types_reach_the_layout() {
    let cls = ClassObj::new_rc("Point");
    cls.declare_field(Rc::from("x"), TypeTag::Int);
    cls.declare_field(Rc::from("label"), TypeTag::Str);
    cls.declare_field(Rc::from("loose"), TypeTag::Dynamic);

    let layout = cls.get_or_compute_layout();
    let tag_of = |name: &str| layout.get_field(name).map(|f| f.type_tag);

    assert_eq!(tag_of("x"), Some(TypeTag::Int));
    assert_eq!(tag_of("label"), Some(TypeTag::Str));
    assert_eq!(tag_of("loose"), Some(TypeTag::Dynamic));

    // Whether the collector has to trace a field follows from its type.
    let gc_of = |name: &str| layout.get_field(name).map(|f| f.is_gc_ref);
    assert_eq!(gc_of("x"), Some(false));
    assert_eq!(gc_of("label"), Some(true));
}

/// Redeclaring a field keeps its slot and takes the newer type: `op_inherit`
/// republishes a superclass field before the subclass declares its own.
#[test]
fn redeclaring_a_field_keeps_its_slot() {
    let cls = ClassObj::new_rc("Redeclared");
    let first = cls.declare_field(Rc::from("v"), TypeTag::Dynamic);
    let second = cls.declare_field(Rc::from("v"), TypeTag::Int);

    assert_eq!(first, second);
    assert_eq!(
        cls.get_or_compute_layout()
            .get_field("v")
            .map(|f| f.type_tag),
        Some(TypeTag::Int)
    );
}
