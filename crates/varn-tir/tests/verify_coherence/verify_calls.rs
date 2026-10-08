use super::fixtures::{expr, int};
use std::sync::Arc;
use varn_tir::*;
#[test]
fn method_call_arity_mismatch_is_rejected() {
    let mut m = TirModule {
        source_file: Arc::from("test.vn"),
        imports: vec![],
        exports: vec![],
        types: TyTable::default(),
        classes: vec![ClassInfo::new_with_methods(
            Arc::from("Point"),
            varn_tir::Ancestry::Root,
            vec![],
            vec![("move".into(), SigId(0))],
            &varn_tir::TyTable::default(),
        )],
        enums: vec![],
        signatures: vec![
            Signature {
                params: vec![BackendTy::Int],
                return_ty: BackendTy::Void,
                has_rest: false,
            },
            Signature {
                params: vec![],
                return_ty: BackendTy::Void,
                has_rest: false,
            },
        ],
        functions: vec![],
        globals: vec![],
        global_names: vec![],
        class_defs: vec![],
        top_level: TirFunction {
            force_inline: false,
            name: Arc::from("<module>"),
            sig: SigId(1),
            params: vec![],
            return_ty: BackendTy::Void,
            locals: vec![BackendTy::Class(ClassId(0))],
            body: vec![],
            has_this: false,
            this_class: None,
            is_async: false,
            is_generator: false,
            has_rest: false,
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
            args: vec![],
        },
        BackendTy::Void,
        Resolution::VtableSlot(0),
    )));
    let errs = verify_module(&m).unwrap_err();
    assert!(
        errs.iter().any(|e| e.message.contains("argument")),
        "got: {:?}",
        errs
    );
}
#[test]
fn method_call_with_correct_arity_verifies() {
    let mut m = TirModule {
        source_file: Arc::from("test.vn"),
        imports: vec![],
        exports: vec![],
        types: TyTable::default(),
        classes: vec![ClassInfo::new_with_methods(
            Arc::from("Point"),
            varn_tir::Ancestry::Root,
            vec![],
            vec![("move".into(), SigId(0))],
            &varn_tir::TyTable::default(),
        )],
        enums: vec![],
        signatures: vec![
            Signature {
                params: vec![BackendTy::Int],
                return_ty: BackendTy::Void,
                has_rest: false,
            },
            Signature {
                params: vec![],
                return_ty: BackendTy::Void,
                has_rest: false,
            },
        ],
        functions: vec![],
        globals: vec![],
        global_names: vec![],
        class_defs: vec![],
        top_level: TirFunction {
            force_inline: false,
            name: Arc::from("<module>"),
            sig: SigId(1),
            params: vec![],
            return_ty: BackendTy::Void,
            locals: vec![BackendTy::Class(ClassId(0))],
            body: vec![],
            has_this: false,
            this_class: None,
            is_async: false,
            is_generator: false,
            has_rest: false,
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
            args: vec![TirArg::Expr(int(42))],
        },
        BackendTy::Void,
        Resolution::VtableSlot(0),
    )));
    assert!(verify_module(&m).is_ok());
}
#[test]
fn method_call_arg_nonnull_into_nullable_param_passes() {
    let mut types = TyTable::default();
    let int_id = types.intern(BackendTy::Int);
    let mut m = TirModule {
        source_file: Arc::from("test.vn"),
        imports: vec![],
        exports: vec![],
        types,
        classes: vec![ClassInfo::new_with_methods(
            Arc::from("Point"),
            varn_tir::Ancestry::Root,
            vec![],
            vec![("move".into(), SigId(0))],
            &varn_tir::TyTable::default(),
        )],
        enums: vec![],
        signatures: vec![
            Signature {
                params: vec![BackendTy::Nullable(int_id)],
                return_ty: BackendTy::Void,
                has_rest: false,
            },
            Signature {
                params: vec![],
                return_ty: BackendTy::Void,
                has_rest: false,
            },
        ],
        functions: vec![],
        globals: vec![],
        global_names: vec![],
        class_defs: vec![],
        top_level: TirFunction {
            force_inline: false,
            name: Arc::from("<module>"),
            sig: SigId(1),
            params: vec![],
            return_ty: BackendTy::Void,
            locals: vec![BackendTy::Class(ClassId(0))],
            body: vec![],
            has_this: false,
            this_class: None,
            is_async: false,
            is_generator: false,
            has_rest: false,
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
            args: vec![TirArg::Expr(int(42))],
        },
        BackendTy::Void,
        Resolution::VtableSlot(0),
    )));
    assert!(verify_module(&m).is_ok());
}
#[test]
fn method_call_arg_nullable_into_nonnull_param_rejected() {
    let mut types = TyTable::default();
    let int_id = types.intern(BackendTy::Int);
    let mut m = TirModule {
        source_file: Arc::from("test.vn"),
        imports: vec![],
        exports: vec![],
        types,
        classes: vec![ClassInfo::new_with_methods(
            Arc::from("Point"),
            varn_tir::Ancestry::Root,
            vec![],
            vec![("move".into(), SigId(0))],
            &varn_tir::TyTable::default(),
        )],
        enums: vec![],
        signatures: vec![
            Signature {
                params: vec![BackendTy::Int],
                return_ty: BackendTy::Void,
                has_rest: false,
            },
            Signature {
                params: vec![],
                return_ty: BackendTy::Void,
                has_rest: false,
            },
        ],
        functions: vec![],
        globals: vec![],
        global_names: vec![],
        class_defs: vec![],
        top_level: TirFunction {
            force_inline: false,
            name: Arc::from("<module>"),
            sig: SigId(1),
            params: vec![],
            return_ty: BackendTy::Void,
            locals: vec![BackendTy::Class(ClassId(0))],
            body: vec![],
            has_this: false,
            this_class: None,
            is_async: false,
            is_generator: false,
            has_rest: false,
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
            args: vec![TirArg::Expr(expr(
                TirExprKind::IntLit(42),
                BackendTy::Nullable(int_id),
                Resolution::None,
            ))],
        },
        BackendTy::Void,
        Resolution::VtableSlot(0),
    )));
    let errs = verify_module(&m).unwrap_err();
    assert!(
        errs.iter().any(|e| e.message.contains("argument")),
        "got: {:?}",
        errs
    );
}
