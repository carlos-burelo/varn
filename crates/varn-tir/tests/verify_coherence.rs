//! A missing type costs performance. A WRONG type is a miscompile, and
//! nothing in the pipeline looks for one today. These checks are the trip
//! wire.

use std::rc::Rc;
use varn_tir::*;

fn module_with_point() -> TirModule {
    let mut types = TyTable::default();
    let _ = types.intern(BackendTy::Int);
    TirModule {
        source_file: Rc::from("test.vn"),
        types,
        classes: vec![ClassInfo::new(
            Rc::from("Point"),
            None,
            vec![("x".into(), BackendTy::Int), ("label".into(), BackendTy::Str)],
        )],
        enums: vec![],
        signatures: vec![Signature { params: vec![], return_ty: BackendTy::Void }],
        functions: vec![],
        globals: vec![],
        top_level: TirFunction {
            name: Rc::from("<module>"),
            sig: SigId(0),
            params: vec![],
            return_ty: BackendTy::Void,
            locals: vec![BackendTy::Class(ClassId(0))],
            body: vec![],
            has_this: false,
            this_class: None,
        },
    }
}

fn expr(kind: TirExprKind, ty: BackendTy, res: Resolution) -> TirExpr {
    TirExpr { kind, ty, res, span: Span::EMPTY }
}

fn int(v: i64) -> TirExpr {
    expr(TirExprKind::IntLit(v), BackendTy::Int, Resolution::None)
}

/// int + int is int.
#[test]
fn int_addition_is_int() {
    let mut m = module_with_point();
    m.top_level.body.push(TirStmt::Expr(expr(
        TirExprKind::Binary { op: TirBinOp::Add, lhs: Box::new(int(1)), rhs: Box::new(int(2)) },
        BackendTy::Int,
        Resolution::None,
    )));
    assert!(verify_module(&m).is_ok());
}

/// int + int claiming to produce Str is a miscompile, and is rejected.
#[test]
fn a_lying_result_type_is_rejected() {
    let mut m = module_with_point();
    m.top_level.body.push(TirStmt::Expr(expr(
        TirExprKind::Binary { op: TirBinOp::Add, lhs: Box::new(int(1)), rhs: Box::new(int(2)) },
        BackendTy::Str,
        Resolution::None,
    )));
    let errs = verify_module(&m).unwrap_err();
    assert!(errs.iter().any(|e| e.message.contains("Add")), "got: {:?}", errs);
}

/// Mixing representations without an explicit Cast is rejected: it is exactly
/// where a float silently travels in an integer register.
#[test]
fn mixed_operands_without_a_cast_are_rejected() {
    let mut m = module_with_point();
    let f = expr(TirExprKind::FloatLit(1.5), BackendTy::Float, Resolution::None);
    m.top_level.body.push(TirStmt::Expr(expr(
        TirExprKind::Binary { op: TirBinOp::Add, lhs: Box::new(int(1)), rhs: Box::new(f) },
        BackendTy::Float,
        Resolution::None,
    )));
    let errs = verify_module(&m).unwrap_err();
    assert!(errs.iter().any(|e| e.message.contains("Cast")), "got: {:?}", errs);
}

/// A comparison produces Bool whatever its operands are.
#[test]
fn comparison_produces_bool() {
    let mut m = module_with_point();
    m.top_level.body.push(TirStmt::Expr(expr(
        TirExprKind::Binary { op: TirBinOp::Lt, lhs: Box::new(int(1)), rhs: Box::new(int(2)) },
        BackendTy::Int, // wrong on purpose
        Resolution::None,
    )));
    let errs = verify_module(&m).unwrap_err();
    assert!(errs.iter().any(|e| e.message.contains("Bool")), "got: {:?}", errs);
}

/// A field read's type must be the field's DECLARED type. This is the check
/// that makes a slot and a type disagreeing impossible.
#[test]
fn a_field_read_must_have_the_declared_type() {
    let mut m = module_with_point();
    let recv = expr(
        TirExprKind::Var,
        BackendTy::Class(ClassId(0)),
        Resolution::Local(LocalId(0)),
    );
    // slot 0 is `x: int`, but the node claims Str.
    m.top_level.body.push(TirStmt::Expr(expr(
        TirExprKind::Field { object: Box::new(recv), name: "x".into() },
        BackendTy::Str,
        Resolution::FieldSlot(0),
    )));
    let errs = verify_module(&m).unwrap_err();
    assert!(errs.iter().any(|e| e.message.contains("declared")), "got: {:?}", errs);
}

/// The same read with the right type passes.
#[test]
fn a_correct_field_read_verifies() {
    let mut m = module_with_point();
    let recv = expr(
        TirExprKind::Var,
        BackendTy::Class(ClassId(0)),
        Resolution::Local(LocalId(0)),
    );
    m.top_level.body.push(TirStmt::Expr(expr(
        TirExprKind::Field { object: Box::new(recv), name: "x".into() },
        BackendTy::Int,
        Resolution::FieldSlot(0),
    )));
    assert!(verify_module(&m).is_ok(), "{:?}", verify_module(&m));
}

/// A method call's argument count must match the signature.
#[test]
fn method_call_arity_mismatch_is_rejected() {
    let mut m = TirModule {
        source_file: Rc::from("test.vn"),
        types: TyTable::default(),
        classes: vec![ClassInfo::new_with_methods(
            Rc::from("Point"),
            None,
            vec![],
            vec![("move".into(), SigId(0))],
        )],
        enums: vec![],
        signatures: vec![
            Signature { params: vec![BackendTy::Int], return_ty: BackendTy::Void },
            Signature { params: vec![], return_ty: BackendTy::Void },
        ],
        functions: vec![],
        globals: vec![],
        top_level: TirFunction {
            name: Rc::from("<module>"),
            sig: SigId(1),
            params: vec![],
            return_ty: BackendTy::Void,
            locals: vec![BackendTy::Class(ClassId(0))],
            body: vec![],
            has_this: false,
            this_class: None,
        },
    };

    let recv = expr(
        TirExprKind::Var,
        BackendTy::Class(ClassId(0)),
        Resolution::Local(LocalId(0)),
    );
    // Passing 0 args when signature expects 1
    m.top_level.body.push(TirStmt::Expr(expr(
        TirExprKind::MethodCall {
            recv: Box::new(recv),
            name: "move".into(),
            args: vec![],
        },
        BackendTy::Void,
        Resolution::VtableSlot(0),
    )));
    let errs = verify_module(&m).unwrap_err();
    assert!(errs.iter().any(|e| e.message.contains("argument")), "got: {:?}", errs);
}

/// A method call with the right arity passes.
#[test]
fn method_call_with_correct_arity_verifies() {
    let mut m = TirModule {
        source_file: Rc::from("test.vn"),
        types: TyTable::default(),
        classes: vec![ClassInfo::new_with_methods(
            Rc::from("Point"),
            None,
            vec![],
            vec![("move".into(), SigId(0))],
        )],
        enums: vec![],
        signatures: vec![
            Signature { params: vec![BackendTy::Int], return_ty: BackendTy::Void },
            Signature { params: vec![], return_ty: BackendTy::Void },
        ],
        functions: vec![],
        globals: vec![],
        top_level: TirFunction {
            name: Rc::from("<module>"),
            sig: SigId(1),
            params: vec![],
            return_ty: BackendTy::Void,
            locals: vec![BackendTy::Class(ClassId(0))],
            body: vec![],
            has_this: false,
            this_class: None,
        },
    };

    let recv = expr(
        TirExprKind::Var,
        BackendTy::Class(ClassId(0)),
        Resolution::Local(LocalId(0)),
    );
    m.top_level.body.push(TirStmt::Expr(expr(
        TirExprKind::MethodCall {
            recv: Box::new(recv),
            name: "move".into(),
            args: vec![int(42)],
        },
        BackendTy::Void,
        Resolution::VtableSlot(0),
    )));
    assert!(verify_module(&m).is_ok());
}

/// An if condition must be Bool.
#[test]
fn if_condition_must_be_bool() {
    let mut m = module_with_point();
    m.top_level.body.push(TirStmt::If {
        cond: int(42), // Int, not Bool
        then_body: vec![],
        else_body: vec![],
    });
    let errs = verify_module(&m).unwrap_err();
    assert!(errs.iter().any(|e| e.message.contains("Bool")), "got: {:?}", errs);
}

/// An if condition that is Bool passes.
#[test]
fn if_with_bool_condition_verifies() {
    let mut m = module_with_point();
    m.top_level.body.push(TirStmt::If {
        cond: expr(TirExprKind::BoolLit(true), BackendTy::Bool, Resolution::None),
        then_body: vec![],
        else_body: vec![],
    });
    assert!(verify_module(&m).is_ok());
}

/// A loop condition must be Bool.
#[test]
fn loop_condition_must_be_bool() {
    let mut m = module_with_point();
    m.top_level.body.push(TirStmt::Loop {
        cond: int(1), // Int, not Bool
        body: vec![],
    });
    let errs = verify_module(&m).unwrap_err();
    assert!(errs.iter().any(|e| e.message.contains("Bool")), "got: {:?}", errs);
}

/// A loop condition that is Bool passes.
#[test]
fn loop_with_bool_condition_verifies() {
    let mut m = module_with_point();
    m.top_level.body.push(TirStmt::Loop {
        cond: expr(TirExprKind::BoolLit(true), BackendTy::Bool, Resolution::None),
        body: vec![],
    });
    assert!(verify_module(&m).is_ok());
}

/// A let binding's type must match the initializer's type.
#[test]
fn let_type_mismatch_is_rejected() {
    let mut m = module_with_point();
    m.top_level.body.push(TirStmt::Let {
        local: LocalId(1),
        ty: BackendTy::Str, // declared Str
        init: Some(int(42)), // initialized with Int
    });
    let errs = verify_module(&m).unwrap_err();
    assert!(errs.iter().any(|e| e.message.contains("declares")), "got: {:?}", errs);
}

/// A let binding with matching types verifies.
#[test]
fn let_with_matching_type_verifies() {
    let mut m = module_with_point();
    m.top_level.body.push(TirStmt::Let {
        local: LocalId(1),
        ty: BackendTy::Int,
        init: Some(int(42)),
    });
    assert!(verify_module(&m).is_ok());
}

/// A return statement's type must match the function's return type.
#[test]
fn return_type_mismatch_is_rejected() {
    let mut m = module_with_point();
    m.top_level.body.push(TirStmt::Return(Some(int(42)))); // Int, but function returns Void
    let errs = verify_module(&m).unwrap_err();
    assert!(errs.iter().any(|e| e.message.contains("return type")), "got: {:?}", errs);
}

/// A return statement with the right type verifies.
#[test]
fn return_with_correct_type_verifies() {
    let mut m = TirModule {
        source_file: Rc::from("test.vn"),
        types: TyTable::default(),
        classes: vec![],
        enums: vec![],
        signatures: vec![Signature { params: vec![], return_ty: BackendTy::Int }],
        functions: vec![],
        globals: vec![],
        top_level: TirFunction {
            name: Rc::from("<module>"),
            sig: SigId(0),
            params: vec![],
            return_ty: BackendTy::Int,
            locals: vec![],
            body: vec![],
            has_this: false,
            this_class: None,
        },
    };
    m.top_level.body.push(TirStmt::Return(Some(int(42))));
    assert!(verify_module(&m).is_ok());
}

/// A bare return in a function that returns Void passes.
#[test]
fn bare_return_in_void_function_verifies() {
    let mut m = module_with_point();
    m.top_level.body.push(TirStmt::Return(None));
    assert!(verify_module(&m).is_ok());
}

/// A bare return in a function that doesn't return Void is rejected.
#[test]
fn bare_return_in_non_void_function_is_rejected() {
    let mut m = TirModule {
        source_file: Rc::from("test.vn"),
        types: TyTable::default(),
        classes: vec![],
        enums: vec![],
        signatures: vec![Signature { params: vec![], return_ty: BackendTy::Int }],
        functions: vec![],
        globals: vec![],
        top_level: TirFunction {
            name: Rc::from("<module>"),
            sig: SigId(0),
            params: vec![],
            return_ty: BackendTy::Int,
            locals: vec![],
            body: vec![],
            has_this: false,
            this_class: None,
        },
    };
    m.top_level.body.push(TirStmt::Return(None));
    let errs = verify_module(&m).unwrap_err();
    assert!(errs.iter().any(|e| e.message.contains("return type")), "got: {:?}", errs);
}
