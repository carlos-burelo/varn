use std::sync::Arc;
use varn_core::RuntimeKind;
use varn_types::value::ClassObj;

/// The declared type of a field reaches the layout the runtime allocates by.
/// It arrives with the declaration — nothing downstream can re-derive it — so
/// a layout reporting a boxed field for a declared `int` means the operand carrying
/// it was dropped somewhere between the compiler and here.
#[test]
fn declared_field_types_reach_the_layout() {
    let cls = ClassObj::new_rc("Point");
    cls.declare_field(Arc::from("x"), Some(RuntimeKind::Int));
    cls.declare_field(Arc::from("label"), Some(RuntimeKind::Str));
    cls.declare_field(Arc::from("loose"), None);

    let layout = cls.get_or_compute_layout();
    let tag_of = |name: &str| layout.get_field(name).map(|f| f.kind);

    assert_eq!(tag_of("x"), Some(Some(RuntimeKind::Int)));
    assert_eq!(tag_of("label"), Some(Some(RuntimeKind::Str)));
    assert_eq!(tag_of("loose"), Some(None));

    // Whether the collector has to trace a field follows from its type.
    let gc_of = |name: &str| {
        layout
            .get_field(name)
            .map(|f| f.layout.repr.holds_reference())
    };
    assert_eq!(gc_of("x"), Some(false));
    assert_eq!(gc_of("label"), Some(true));
}

/// Redeclaring a field keeps its slot and takes the newer type: `op_inherit`
/// republishes a superclass field before the subclass declares its own.
#[test]
fn redeclaring_a_field_keeps_its_slot() {
    let cls = ClassObj::new_rc("Redeclared");
    let first = cls.declare_field(Arc::from("v"), None);
    let second = cls.declare_field(Arc::from("v"), Some(RuntimeKind::Int));

    assert_eq!(first, second);
    assert_eq!(
        cls.get_or_compute_layout()
            .get_field("v")
            .and_then(|f| f.kind),
        Some(RuntimeKind::Int)
    );
}
