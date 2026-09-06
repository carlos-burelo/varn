//! Well-formedness: every handle points at something that exists, and every
//! slot is in range of the table it claims to index. These are the checks
//! that make a dangling ClassId or an out-of-range vtable slot impossible
//! rather than improbable.

use std::rc::Rc;
use varn_tir::*;

fn empty_module() -> TirModule {
    TirModule {
        source_file: Rc::from("test.vn"),
        types: TyTable::default(),
        classes: vec![ClassInfo::new(Rc::from("P"), None, vec![("x".into(), BackendTy::Int)])],
        enums: vec![],
        signatures: vec![Signature { params: vec![], return_ty: BackendTy::Void }],
        functions: vec![],
        globals: vec![],
        top_level: TirFunction {
            name: Rc::from("<module>"),
            sig: SigId(0),
            params: vec![],
            return_ty: BackendTy::Void,
            locals: vec![],
            body: vec![],
            has_this: false,
            this_class: None,
        },
    }
}

fn expr(kind: TirExprKind, ty: BackendTy, res: Resolution) -> TirExpr {
    TirExpr { kind, ty, res, span: Span::EMPTY }
}

/// A module with nothing wrong passes.
#[test]
fn an_empty_module_verifies() {
    assert!(verify_module(&empty_module()).is_ok());
}

/// A ClassId with no entry is rejected. Today nothing checks this.
#[test]
fn a_dangling_class_id_is_rejected() {
    let mut m = empty_module();
    m.top_level.body.push(TirStmt::Expr(expr(
        TirExprKind::New { class: ClassId(99), args: vec![] },
        BackendTy::Class(ClassId(99)),
        Resolution::None,
    )));
    let errs = verify_module(&m).unwrap_err();
    assert!(
        errs.iter().any(|e| e.message.contains("ClassId")),
        "expected a dangling-class error, got: {:?}",
        errs
    );
}

/// A field slot past the end of the class's layout is rejected.
#[test]
fn an_out_of_range_field_slot_is_rejected() {
    let mut m = empty_module();
    let recv = expr(TirExprKind::Var, BackendTy::Class(ClassId(0)), Resolution::Local(LocalId(0)));
    m.top_level.locals.push(BackendTy::Class(ClassId(0)));
    m.top_level.body.push(TirStmt::Expr(expr(
        TirExprKind::Field { object: Box::new(recv), name: "nope".into() },
        BackendTy::Int,
        Resolution::FieldSlot(7), // the class has one field
    )));
    let errs = verify_module(&m).unwrap_err();
    assert!(
        errs.iter().any(|e| e.message.contains("out of range")),
        "expected an out-of-range field slot error, got: {:?}",
        errs
    );
}

/// A field slot on a non-class receiver is rejected.
#[test]
fn a_field_slot_on_non_class_receiver_is_rejected() {
    let mut m = empty_module();
    let recv = expr(TirExprKind::Var, BackendTy::Int, Resolution::Local(LocalId(0)));
    m.top_level.locals.push(BackendTy::Int);
    m.top_level.body.push(TirStmt::Expr(expr(
        TirExprKind::Field { object: Box::new(recv), name: "nope".into() },
        BackendTy::Int,
        Resolution::FieldSlot(0),
    )));
    let errs = verify_module(&m).unwrap_err();
    assert!(
        errs.iter().any(|e| e.message.contains("not a class")),
        "expected a non-class receiver error, got: {:?}",
        errs
    );
}

/// A vtable slot past the end of the class's vtable is rejected.
#[test]
fn an_out_of_range_vtable_slot_is_rejected() {
    let mut m = empty_module();
    let recv = expr(TirExprKind::Var, BackendTy::Class(ClassId(0)), Resolution::Local(LocalId(0)));
    m.top_level.locals.push(BackendTy::Class(ClassId(0)));
    m.top_level.body.push(TirStmt::Expr(expr(
        TirExprKind::MethodCall { recv: Box::new(recv), name: "m".into(), args: vec![] },
        BackendTy::Void,
        Resolution::VtableSlot(3), // the class has no methods
    )));
    let errs = verify_module(&m).unwrap_err();
    assert!(
        errs.iter().any(|e| e.message.contains("out of range")),
        "expected an out-of-range vtable error, got: {:?}",
        errs
    );
}

/// A vtable slot on a non-class receiver is rejected.
#[test]
fn a_vtable_slot_on_non_class_receiver_is_rejected() {
    let mut m = empty_module();
    let recv = expr(TirExprKind::Var, BackendTy::Int, Resolution::Local(LocalId(0)));
    m.top_level.locals.push(BackendTy::Int);
    m.top_level.body.push(TirStmt::Expr(expr(
        TirExprKind::MethodCall { recv: Box::new(recv), name: "m".into(), args: vec![] },
        BackendTy::Void,
        Resolution::VtableSlot(0),
    )));
    let errs = verify_module(&m).unwrap_err();
    assert!(
        errs.iter().any(|e| e.message.contains("not a class")),
        "expected a non-class receiver error, got: {:?}",
        errs
    );
}

/// Self-referential types do not cause the verifier to hang.
#[test]
fn cyclic_types_are_handled() {
    let mut m = empty_module();
    // Create a cyclic type: TyId(0) = Array(TyId(1)), TyId(1) = Nullable(TyId(0))
    let mut types = TyTable::default();
    let _ = types.intern(BackendTy::Array(TyId(1)));
    let _ = types.intern(BackendTy::Nullable(TyId(0)));
    m.types = types;

    // Expression with the cyclic type
    m.top_level.body.push(TirStmt::Expr(expr(
        TirExprKind::IntLit(42),
        BackendTy::Array(TyId(1)),
        Resolution::None,
    )));

    // Should complete without hanging; the error doesn't matter for this test
    let _ = verify_module(&m);
}

/// Local resolution is validated against the function's locals.
#[test]
fn out_of_range_local_is_rejected() {
    let mut m = empty_module();
    m.top_level.body.push(TirStmt::Expr(expr(
        TirExprKind::Var,
        BackendTy::Int,
        Resolution::Local(LocalId(5)), // out of range
    )));
    let errs = verify_module(&m).unwrap_err();
    assert!(
        errs.iter().any(|e| e.message.contains("LocalId") && e.message.contains("out of range")),
        "expected an out-of-range local error, got: {:?}",
        errs
    );
}

/// Parameter resolution is validated against the function's parameters.
#[test]
fn out_of_range_param_is_rejected() {
    let mut m = empty_module();
    m.top_level.body.push(TirStmt::Expr(expr(
        TirExprKind::Var,
        BackendTy::Int,
        Resolution::Param(5), // out of range
    )));
    let errs = verify_module(&m).unwrap_err();
    assert!(
        errs.iter().any(|e| e.message.contains("parameter") && e.message.contains("out of range")),
        "expected an out-of-range parameter error, got: {:?}",
        errs
    );
}

/// Tuple elements are checked for dangling types.
#[test]
fn dangling_type_in_tuple_is_rejected() {
    let mut m = empty_module();
    let mut types = TyTable::default();
    // Create a tuple containing a dangling class: Tuple([Class(ClassId(99))])
    let list_id = types.intern_list(&[BackendTy::Class(ClassId(99))]);
    let _ = types.intern(BackendTy::Tuple(list_id));
    m.types = types;

    // Expression with the tuple type
    m.top_level.body.push(TirStmt::Expr(expr(
        TirExprKind::IntLit(42),
        BackendTy::Tuple(list_id),
        Resolution::None,
    )));

    let errs = verify_module(&m).unwrap_err();
    assert!(
        errs.iter().any(|e| e.message.contains("ClassId(99)") && e.message.contains("no entry")),
        "expected a dangling class in tuple error, got: {:?}",
        errs
    );
}

/// Cyclic tuples do not cause the verifier to hang.
#[test]
fn cyclic_tuple_does_not_hang() {
    let mut m = empty_module();
    // Create a cycle through indirection:
    // TyId(0) = Array(TyId(1))
    // TyId(1) = Tuple([Array(TyId(1))])
    // This cycles because Tuple's element is Array(TyId(1)), which is TyId(0)
    let mut types = TyTable::default();
    let _ = types.intern(BackendTy::Array(TyId(1))); // TyId(0)
    let list_id = types.intern_list(&[BackendTy::Array(TyId(1))]); // TyListId(0)
    let _ = types.intern(BackendTy::Tuple(list_id)); // TyId(1)
    m.types = types;

    // Expression with a type that eventually cycles back
    m.top_level.body.push(TirStmt::Expr(expr(
        TirExprKind::IntLit(42),
        BackendTy::Array(TyId(1)), // Array(Tuple([Array(TyId(1))]))... cycles
        Resolution::None,
    )));

    // Should complete without hanging
    let _ = verify_module(&m);
}

