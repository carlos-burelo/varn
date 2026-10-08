use super::fixtures::{expr, int, module_with_point};
use std::sync::Arc;
use varn_tir::*;
#[test]
fn bare_null_returns_from_any_nullable_function() {
    let mut m = module_with_point();
    let never_id = m.types.intern(BackendTy::Never);
    let int_id = m.types.intern(BackendTy::Int);
    m.signatures.push(Signature {
        params: vec![],
        return_ty: BackendTy::Nullable(int_id),
        has_rest: false,
    });
    m.functions.push(TirFunction {
        force_inline: false,
        name: Arc::from("first"),
        sig: SigId(1),
        params: vec![],
        return_ty: BackendTy::Nullable(int_id),
        locals: vec![],
        body: vec![TirStmt::Return(Some(expr(
            TirExprKind::NullLit,
            BackendTy::Nullable(never_id),
            Resolution::None,
        )))],
        has_this: false,
        this_class: None,
        is_async: false,
        is_generator: false,
        has_rest: false,
    });
    assert!(verify_module(&m).is_ok(), "{:?}", verify_module(&m));
}
#[test]
fn int_argument_widens_to_a_float_parameter() {
    let mut m = module_with_point();
    m.signatures.push(Signature {
        params: vec![BackendTy::Float],
        return_ty: BackendTy::Void,
        has_rest: false,
    });
    m.functions.push(TirFunction {
        force_inline: false,
        name: Arc::from("takesFloat"),
        sig: SigId(1),
        params: vec![BackendTy::Float],
        return_ty: BackendTy::Void,
        locals: vec![],
        body: vec![],
        has_this: false,
        this_class: None,
        is_async: false,
        is_generator: false,
        has_rest: false,
    });
    m.top_level.body.push(TirStmt::Expr(expr(
        TirExprKind::Call {
            callee: Box::new(expr(TirExprKind::Var, BackendTy::Void, Resolution::None)),
            args: vec![TirArg::Expr(int(1))],
        },
        BackendTy::Void,
        Resolution::DirectFn(FnId(0)),
    )));
    assert!(verify_module(&m).is_ok(), "{:?}", verify_module(&m));
}
#[test]
fn a_subclass_is_assignable_to_its_parent() {
    let animal = ClassInfo::new(
        Arc::from("Animal"),
        varn_tir::Ancestry::Root,
        vec![],
        &varn_tir::TyTable::default(),
    );
    let mut dog = ClassInfo::new(
        Arc::from("Dog"),
        varn_tir::Ancestry::Root,
        vec![],
        &varn_tir::TyTable::default(),
    );
    dog.parent = Some(ClassId(0));
    let mut m = module_with_point();
    m.classes = vec![animal, dog];
    m.signatures.push(Signature {
        params: vec![BackendTy::Class(ClassId(0))],
        return_ty: BackendTy::Void,
        has_rest: false,
    });
    m.functions.push(TirFunction {
        force_inline: false,
        name: Arc::from("greet"),
        sig: SigId(1),
        params: vec![BackendTy::Class(ClassId(0))],
        return_ty: BackendTy::Void,
        locals: vec![BackendTy::Class(ClassId(1))],
        body: vec![],
        has_this: false,
        this_class: None,
        is_async: false,
        is_generator: false,
        has_rest: false,
    });
    m.top_level.body.push(TirStmt::Expr(expr(
        TirExprKind::Call {
            callee: Box::new(expr(TirExprKind::Var, BackendTy::Void, Resolution::None)),
            args: vec![TirArg::Expr(expr(
                TirExprKind::Var,
                BackendTy::Class(ClassId(1)),
                Resolution::None,
            ))],
        },
        BackendTy::Void,
        Resolution::DirectFn(FnId(0)),
    )));
    assert!(verify_module(&m).is_ok(), "{:?}", verify_module(&m));
}
#[test]
fn int_addition_is_int() {
    let mut m = module_with_point();
    m.top_level.body.push(TirStmt::Expr(expr(
        TirExprKind::Binary {
            op: TirBinOp::Add,
            lhs: Box::new(int(1)),
            rhs: Box::new(int(2)),
        },
        BackendTy::Int,
        Resolution::None,
    )));
    assert!(verify_module(&m).is_ok());
}
#[test]
fn a_lying_result_type_is_rejected() {
    let mut m = module_with_point();
    m.top_level.body.push(TirStmt::Expr(expr(
        TirExprKind::Binary {
            op: TirBinOp::Add,
            lhs: Box::new(int(1)),
            rhs: Box::new(int(2)),
        },
        BackendTy::Str,
        Resolution::None,
    )));
    let errs = verify_module(&m).unwrap_err();
    assert!(
        errs.iter().any(|e| e.message.contains("Add")),
        "got: {:?}",
        errs
    );
}
#[test]
fn mixed_operands_without_a_cast_are_rejected() {
    let mut m = module_with_point();
    let f = expr(
        TirExprKind::FloatLit(1.5),
        BackendTy::Float,
        Resolution::None,
    );
    m.top_level.body.push(TirStmt::Expr(expr(
        TirExprKind::Binary {
            op: TirBinOp::Add,
            lhs: Box::new(int(1)),
            rhs: Box::new(f),
        },
        BackendTy::Float,
        Resolution::None,
    )));
    let errs = verify_module(&m).unwrap_err();
    assert!(
        errs.iter().any(|e| e.message.contains("Cast")),
        "got: {:?}",
        errs
    );
}
#[test]
fn comparison_produces_bool() {
    let mut m = module_with_point();
    m.top_level.body.push(TirStmt::Expr(expr(
        TirExprKind::Binary {
            op: TirBinOp::Lt,
            lhs: Box::new(int(1)),
            rhs: Box::new(int(2)),
        },
        BackendTy::Int,
        Resolution::None,
    )));
    let errs = verify_module(&m).unwrap_err();
    assert!(
        errs.iter().any(|e| e.message.contains("Bool")),
        "got: {:?}",
        errs
    );
}
#[test]
fn a_field_read_must_have_the_declared_type() {
    let mut m = module_with_point();
    let recv = expr(
        TirExprKind::Var,
        BackendTy::Class(ClassId(0)),
        Resolution::Local(LocalId(0)),
    );

    m.top_level.body.push(TirStmt::Expr(expr(
        TirExprKind::Field {
            object: Box::new(recv),
            name: "x".into(),
        },
        BackendTy::Str,
        Resolution::FieldSlot(0),
    )));
    let errs = verify_module(&m).unwrap_err();
    assert!(
        errs.iter().any(|e| e.message.contains("declared")),
        "got: {:?}",
        errs
    );
}
#[test]
fn a_correct_field_read_verifies() {
    let mut m = module_with_point();
    let recv = expr(
        TirExprKind::Var,
        BackendTy::Class(ClassId(0)),
        Resolution::Local(LocalId(0)),
    );
    m.top_level.body.push(TirStmt::Expr(expr(
        TirExprKind::Field {
            object: Box::new(recv),
            name: "x".into(),
        },
        BackendTy::Int,
        Resolution::FieldSlot(0),
    )));
    assert!(verify_module(&m).is_ok(), "{:?}", verify_module(&m));
}
