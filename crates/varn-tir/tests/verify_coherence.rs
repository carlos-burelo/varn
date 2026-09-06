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
