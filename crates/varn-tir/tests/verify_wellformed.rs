#![allow(unused_crate_dependencies)]

use std::sync::Arc;
use varn_tir::*;

fn empty_module() -> TirModule {
    TirModule {
        source_file: Arc::from("test.vn"),
        imports: vec![],
        exports: vec![],
        types: TyTable::default(),
        classes: vec![ClassInfo::new(
            Arc::from("P"),
            varn_tir::Ancestry::Root,
            vec![("x".into(), BackendTy::Int)],
            &varn_tir::TyTable::default(),
        )],
        enums: vec![],
        signatures: vec![Signature {
            params: vec![],
            return_ty: BackendTy::Void,
            has_rest: false,
        }],
        functions: vec![],
        globals: vec![],
        global_names: vec![],
        class_defs: vec![],
        top_level: TirFunction {
            name: Arc::from("<module>"),
            sig: SigId(0),
            params: vec![],
            return_ty: BackendTy::Void,
            locals: vec![],
            body: vec![],
            has_this: false,
            this_class: None,
            is_async: false,
            is_generator: false,
            has_rest: false,
            force_inline: false,
        },
    }
}

fn expr(kind: TirExprKind, ty: BackendTy, res: Resolution) -> TirExpr {
    TirExpr {
        kind,
        ty,
        res,
        span: Span::EMPTY,
    }
}

#[test]
fn an_empty_module_verifies() {
    assert!(verify_module(&empty_module()).is_ok());
}

#[test]
fn a_dangling_class_id_is_rejected() {
    let mut m = empty_module();
    m.top_level.body.push(TirStmt::Expr(expr(
        TirExprKind::New {
            class: ClassId(99),
            args: vec![],
        },
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

#[test]
fn an_out_of_range_field_slot_is_rejected() {
    let mut m = empty_module();
    let recv = expr(
        TirExprKind::Var,
        BackendTy::Class(ClassId(0)),
        Resolution::Local(LocalId(0)),
    );
    m.top_level.locals.push(BackendTy::Class(ClassId(0)));
    m.top_level.body.push(TirStmt::Expr(expr(
        TirExprKind::Field {
            object: Box::new(recv),
            name: "nope".into(),
        },
        BackendTy::Int,
        Resolution::FieldSlot(7),
    )));
    let errs = verify_module(&m).unwrap_err();
    assert!(
        errs.iter().any(|e| e.message.contains("out of range")),
        "expected an out-of-range field slot error, got: {:?}",
        errs
    );
}

#[test]
fn a_field_slot_on_non_class_receiver_is_rejected() {
    let mut m = empty_module();
    let recv = expr(
        TirExprKind::Var,
        BackendTy::Int,
        Resolution::Local(LocalId(0)),
    );
    m.top_level.locals.push(BackendTy::Int);
    m.top_level.body.push(TirStmt::Expr(expr(
        TirExprKind::Field {
            object: Box::new(recv),
            name: "nope".into(),
        },
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

#[test]
fn an_out_of_range_vtable_slot_is_rejected() {
    let mut m = empty_module();
    let recv = expr(
        TirExprKind::Var,
        BackendTy::Class(ClassId(0)),
        Resolution::Local(LocalId(0)),
    );
    m.top_level.locals.push(BackendTy::Class(ClassId(0)));
    m.top_level.body.push(TirStmt::Expr(expr(
        TirExprKind::MethodCall {
            recv: Box::new(recv),
            name: "m".into(),
            args: vec![],
        },
        BackendTy::Void,
        Resolution::VtableSlot(3),
    )));
    let errs = verify_module(&m).unwrap_err();
    assert!(
        errs.iter().any(|e| e.message.contains("out of range")),
        "expected an out-of-range vtable error, got: {:?}",
        errs
    );
}

#[test]
fn a_vtable_slot_on_non_class_receiver_is_rejected() {
    let mut m = empty_module();
    let recv = expr(
        TirExprKind::Var,
        BackendTy::Int,
        Resolution::Local(LocalId(0)),
    );
    m.top_level.locals.push(BackendTy::Int);
    m.top_level.body.push(TirStmt::Expr(expr(
        TirExprKind::MethodCall {
            recv: Box::new(recv),
            name: "m".into(),
            args: vec![],
        },
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

#[test]
fn cyclic_types_are_handled() {
    let mut m = empty_module();

    let mut types = TyTable::default();
    let _ = types.intern(BackendTy::Array(TyId(1)));
    let _ = types.intern(BackendTy::Nullable(TyId(0)));
    m.types = types;

    m.top_level.body.push(TirStmt::Expr(expr(
        TirExprKind::IntLit(42),
        BackendTy::Array(TyId(1)),
        Resolution::None,
    )));

    let _ = verify_module(&m);
}

#[test]
fn out_of_range_local_is_rejected() {
    let mut m = empty_module();
    m.top_level.body.push(TirStmt::Expr(expr(
        TirExprKind::Var,
        BackendTy::Int,
        Resolution::Local(LocalId(5)),
    )));
    let errs = verify_module(&m).unwrap_err();
    assert!(
        errs.iter()
            .any(|e| e.message.contains("LocalId") && e.message.contains("out of range")),
        "expected an out-of-range local error, got: {:?}",
        errs
    );
}

#[test]
fn out_of_range_param_is_rejected() {
    let mut m = empty_module();
    m.top_level.body.push(TirStmt::Expr(expr(
        TirExprKind::Var,
        BackendTy::Int,
        Resolution::Param(5),
    )));
    let errs = verify_module(&m).unwrap_err();
    assert!(
        errs.iter()
            .any(|e| e.message.contains("parameter") && e.message.contains("out of range")),
        "expected an out-of-range parameter error, got: {:?}",
        errs
    );
}

#[test]
fn dangling_type_in_tuple_is_rejected() {
    let mut m = empty_module();
    let mut types = TyTable::default();

    let list_id = types.intern_list(&[BackendTy::Class(ClassId(99))]);
    let _ = types.intern(BackendTy::Tuple(list_id));
    m.types = types;

    m.top_level.body.push(TirStmt::Expr(expr(
        TirExprKind::IntLit(42),
        BackendTy::Tuple(list_id),
        Resolution::None,
    )));

    let errs = verify_module(&m).unwrap_err();
    assert!(
        errs.iter()
            .any(|e| e.message.contains("ClassId(99)") && e.message.contains("no entry")),
        "expected a dangling class in tuple error, got: {:?}",
        errs
    );
}

#[test]
fn cyclic_tuple_does_not_hang() {
    let mut m = empty_module();

    let mut types = TyTable::default();
    let _ = types.intern(BackendTy::Array(TyId(1)));
    let list_id = types.intern_list(&[BackendTy::Array(TyId(1))]);
    let _ = types.intern(BackendTy::Tuple(list_id));
    m.types = types;

    m.top_level.body.push(TirStmt::Expr(expr(
        TirExprKind::IntLit(42),
        BackendTy::Array(TyId(1)),
        Resolution::None,
    )));

    let _ = verify_module(&m);
}

#[test]
fn a_dangling_type_handle_on_a_field_object_does_not_panic() {
    let mut m = empty_module();
    let recv = expr(
        TirExprKind::Var,
        BackendTy::Nullable(TyId(999)),
        Resolution::Local(LocalId(0)),
    );
    m.top_level.locals.push(BackendTy::Nullable(TyId(999)));
    m.top_level.body.push(TirStmt::Expr(expr(
        TirExprKind::Field {
            object: Box::new(recv),
            name: "x".into(),
        },
        BackendTy::Int,
        Resolution::FieldSlot(0),
    )));

    let errs = verify_module(&m).unwrap_err();
    assert!(
        errs.iter().any(|e| e.message.contains("TyId(999)")),
        "expected a dangling-TyId error, got: {:?}",
        errs
    );
}

#[test]
fn self_referential_tuple_list_does_not_overflow() {
    let mut m = empty_module();
    let mut types = TyTable::default();
    let list_id = types.intern_list(&[BackendTy::Tuple(TyListId(0))]);
    assert_eq!(list_id, TyListId(0), "must be genuinely self-referential");
    m.types = types;

    m.top_level.body.push(TirStmt::Expr(expr(
        TirExprKind::IntLit(42),
        BackendTy::Tuple(list_id),
        Resolution::None,
    )));

    let _ = verify_module(&m);
}

#[test]
fn out_of_range_let_local_is_rejected() {
    let mut m = empty_module();
    m.top_level.body.push(TirStmt::Let {
        local: LocalId(5),
        ty: BackendTy::Int,
        init: None,
    });
    let errs = verify_module(&m).unwrap_err();
    assert!(
        errs.iter()
            .any(|e| e.message.contains("LocalId") && e.message.contains("out of range")),
        "expected an out-of-range Let local error, got: {:?}",
        errs
    );
}

#[test]
fn out_of_range_try_catch_local_is_rejected() {
    let mut m = empty_module();
    m.top_level.body.push(TirStmt::Try {
        body: vec![],
        catch_local: LocalId(5),
        catch_body: vec![],
    });
    let errs = verify_module(&m).unwrap_err();
    assert!(
        errs.iter()
            .any(|e| e.message.contains("LocalId") && e.message.contains("out of range")),
        "expected an out-of-range Try catch local error, got: {:?}",
        errs
    );
}

#[test]
fn a_dangling_vtable_sig_is_rejected() {
    let mut m = empty_module();
    m.classes = vec![ClassInfo::new_with_methods(
        Arc::from("P"),
        varn_tir::Ancestry::Root,
        vec![("x".into(), BackendTy::Int)],
        vec![("m".into(), SigId(99))],
        &varn_tir::TyTable::default(),
    )];
    let errs = verify_module(&m).unwrap_err();
    assert!(
        errs.iter().any(|e| e.message.contains("SigId(99)")),
        "expected a dangling vtable SigId error, got: {:?}",
        errs
    );
}
