#![allow(unused_crate_dependencies)]

use std::sync::Arc;
use varn_tir::*;

fn module_with_point() -> TirModule {
    let mut types = TyTable::default();
    let _ = types.intern(BackendTy::Int);
    TirModule {
        source_file: Arc::from("test.vn"),
        imports: vec![],
        exports: vec![],
        types,
        classes: vec![ClassInfo::new(
            Arc::from("Point"),
            varn_tir::Ancestry::Root,
            vec![
                ("x".into(), BackendTy::Int),
                ("label".into(), BackendTy::Str),
            ],
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
            locals: vec![BackendTy::Class(ClassId(0))],
            body: vec![],
            has_this: false,
            this_class: None,
            is_async: false,
            is_generator: false,
            has_rest: false,
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

fn int(v: i64) -> TirExpr {
    expr(TirExprKind::IntLit(v), BackendTy::Int, Resolution::None)
}

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
fn if_condition_must_be_bool() {
    let mut m = module_with_point();
    m.top_level.body.push(TirStmt::If {
        cond: int(42),
        then_body: vec![],
        else_body: vec![],
    });
    let errs = verify_module(&m).unwrap_err();
    assert!(
        errs.iter().any(|e| e.message.contains("Bool")),
        "got: {:?}",
        errs
    );
}

#[test]
fn if_with_bool_condition_verifies() {
    let mut m = module_with_point();
    m.top_level.body.push(TirStmt::If {
        cond: expr(
            TirExprKind::BoolLit(true),
            BackendTy::Bool,
            Resolution::None,
        ),
        then_body: vec![],
        else_body: vec![],
    });
    assert!(verify_module(&m).is_ok());
}

#[test]
fn loop_condition_must_be_bool() {
    let mut m = module_with_point();
    m.top_level.body.push(TirStmt::Loop {
        cond: int(1),
        body: vec![],
    });
    let errs = verify_module(&m).unwrap_err();
    assert!(
        errs.iter().any(|e| e.message.contains("Bool")),
        "got: {:?}",
        errs
    );
}

#[test]
fn loop_with_bool_condition_verifies() {
    let mut m = module_with_point();
    m.top_level.body.push(TirStmt::Loop {
        cond: expr(
            TirExprKind::BoolLit(true),
            BackendTy::Bool,
            Resolution::None,
        ),
        body: vec![],
    });
    assert!(verify_module(&m).is_ok());
}

#[test]
fn let_type_mismatch_is_rejected() {
    let mut m = module_with_point();
    m.top_level.body.push(TirStmt::Let {
        local: LocalId(1),
        ty: BackendTy::Str,
        init: Some(int(42)),
    });
    let errs = verify_module(&m).unwrap_err();
    assert!(
        errs.iter().any(|e| e.message.contains("declares")),
        "got: {:?}",
        errs
    );
}

#[test]
fn let_with_matching_type_verifies() {
    let mut m = module_with_point();
    m.top_level.locals.push(BackendTy::Int);
    m.top_level.body.push(TirStmt::Let {
        local: LocalId(1),
        ty: BackendTy::Int,
        init: Some(int(42)),
    });
    assert!(verify_module(&m).is_ok());
}

#[test]
fn return_type_mismatch_is_rejected() {
    let mut m = module_with_point();
    m.top_level.body.push(TirStmt::Return(Some(int(42))));
    let errs = verify_module(&m).unwrap_err();
    assert!(
        errs.iter().any(|e| e.message.contains("return type")),
        "got: {:?}",
        errs
    );
}

#[test]
fn return_with_correct_type_verifies() {
    let mut m = TirModule {
        source_file: Arc::from("test.vn"),
        imports: vec![],
        exports: vec![],
        types: TyTable::default(),
        classes: vec![],
        enums: vec![],
        signatures: vec![Signature {
            params: vec![],
            return_ty: BackendTy::Int,
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
            return_ty: BackendTy::Int,
            locals: vec![],
            body: vec![],
            has_this: false,
            this_class: None,
            is_async: false,
            is_generator: false,
            has_rest: false,
        },
    };
    m.top_level.body.push(TirStmt::Return(Some(int(42))));
    assert!(verify_module(&m).is_ok());
}

#[test]
fn bare_return_in_void_function_verifies() {
    let mut m = module_with_point();
    m.top_level.body.push(TirStmt::Return(None));
    assert!(verify_module(&m).is_ok());
}

#[test]
fn bare_return_in_non_void_function_is_rejected() {
    let mut m = TirModule {
        source_file: Arc::from("test.vn"),
        imports: vec![],
        exports: vec![],
        types: TyTable::default(),
        classes: vec![],
        enums: vec![],
        signatures: vec![Signature {
            params: vec![],
            return_ty: BackendTy::Int,
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
            return_ty: BackendTy::Int,
            locals: vec![],
            body: vec![],
            has_this: false,
            this_class: None,
            is_async: false,
            is_generator: false,
            has_rest: false,
        },
    };
    m.top_level.body.push(TirStmt::Return(None));
    let errs = verify_module(&m).unwrap_err();
    assert!(
        errs.iter().any(|e| e.message.contains("return type")),
        "got: {:?}",
        errs
    );
}

#[test]
fn let_with_nullable_declared_and_nonnull_init_is_valid() {
    let mut types = TyTable::default();
    let int_id = types.intern(BackendTy::Int);
    let nullable_int = BackendTy::Nullable(int_id);
    let mut m = TirModule {
        source_file: Arc::from("test.vn"),
        imports: vec![],
        exports: vec![],
        types,
        classes: vec![],
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
            locals: vec![nullable_int],
            body: vec![],
            has_this: false,
            this_class: None,
            is_async: false,
            is_generator: false,
            has_rest: false,
        },
    };
    m.top_level.body.push(TirStmt::Let {
        local: LocalId(0),
        ty: nullable_int,
        init: Some(int(42)),
    });
    assert!(verify_module(&m).is_ok());
}

#[test]
fn let_with_nonnull_declared_and_nullable_init_is_rejected() {
    let mut types = TyTable::default();
    let int_id = types.intern(BackendTy::Int);
    let nullable_int = BackendTy::Nullable(int_id);
    let mut m = TirModule {
        source_file: Arc::from("test.vn"),
        imports: vec![],
        exports: vec![],
        types,
        classes: vec![],
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
        },
    };
    m.top_level.body.push(TirStmt::Let {
        local: LocalId(0),
        ty: BackendTy::Int,
        init: Some(expr(
            TirExprKind::IntLit(42),
            nullable_int,
            Resolution::None,
        )),
    });
    let errs = verify_module(&m).unwrap_err();
    assert!(
        errs.iter().any(|e| e.message.contains("declares")),
        "got: {:?}",
        errs
    );
}

#[test]
fn return_of_never_type_is_valid_anywhere() {
    let mut m = TirModule {
        source_file: Arc::from("test.vn"),
        imports: vec![],
        exports: vec![],
        types: TyTable::default(),
        classes: vec![],
        enums: vec![],
        signatures: vec![Signature {
            params: vec![],
            return_ty: BackendTy::Int,
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
            return_ty: BackendTy::Int,
            locals: vec![],
            body: vec![],
            has_this: false,
            this_class: None,
            is_async: false,
            is_generator: false,
            has_rest: false,
        },
    };
    m.top_level.body.push(TirStmt::Return(Some(expr(
        TirExprKind::IntLit(0),
        BackendTy::Never,
        Resolution::None,
    ))));
    assert!(verify_module(&m).is_ok());
}

#[test]
fn return_nonnull_when_function_returns_nullable_is_valid() {
    let mut types = TyTable::default();
    let int_id = types.intern(BackendTy::Int);
    let mut m = TirModule {
        source_file: Arc::from("test.vn"),
        imports: vec![],
        exports: vec![],
        types,
        classes: vec![],
        enums: vec![],
        signatures: vec![Signature {
            params: vec![],
            return_ty: BackendTy::Nullable(int_id),
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
            return_ty: BackendTy::Nullable(int_id),
            locals: vec![],
            body: vec![],
            has_this: false,
            this_class: None,
            is_async: false,
            is_generator: false,
            has_rest: false,
        },
    };
    m.top_level.body.push(TirStmt::Return(Some(int(42))));
    assert!(verify_module(&m).is_ok());
}

#[test]
fn return_nullable_when_function_returns_nonnull_is_rejected() {
    let mut types = TyTable::default();
    let int_id = types.intern(BackendTy::Int);
    let mut m = TirModule {
        source_file: Arc::from("test.vn"),
        imports: vec![],
        exports: vec![],
        types,
        classes: vec![],
        enums: vec![],
        signatures: vec![Signature {
            params: vec![],
            return_ty: BackendTy::Int,
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
            return_ty: BackendTy::Int,
            locals: vec![],
            body: vec![],
            has_this: false,
            this_class: None,
            is_async: false,
            is_generator: false,
            has_rest: false,
        },
    };
    m.top_level.body.push(TirStmt::Return(Some(expr(
        TirExprKind::IntLit(42),
        BackendTy::Nullable(int_id),
        Resolution::None,
    ))));
    let errs = verify_module(&m).unwrap_err();
    assert!(
        errs.iter().any(|e| e.message.contains("return type")),
        "got: {:?}",
        errs
    );
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

#[test]
fn let_declared_nullable_int_init_str_should_fail() {
    let mut types = TyTable::default();
    let int_id = types.intern(BackendTy::Int);
    let nullable_int = BackendTy::Nullable(int_id);
    let mut m = TirModule {
        source_file: Arc::from("test.vn"),
        imports: vec![],
        exports: vec![],
        types,
        classes: vec![],
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
        },
    };
    m.top_level.body.push(TirStmt::Let {
        local: LocalId(0),
        ty: nullable_int,
        init: Some(expr(
            TirExprKind::StrLit("hello".into()),
            BackendTy::Str,
            Resolution::None,
        )),
    });
    let errs = verify_module(&m).unwrap_err();
    assert!(
        errs.iter().any(|e| e.message.contains("declares")),
        "got: {:?}",
        errs
    );
}

#[test]
fn let_declared_nullable_int_init_nullable_str_should_fail() {
    let mut types = TyTable::default();
    let int_id = types.intern(BackendTy::Int);
    let str_id = types.intern(BackendTy::Str);
    let nullable_int = BackendTy::Nullable(int_id);
    let nullable_str = BackendTy::Nullable(str_id);
    let mut m = TirModule {
        source_file: Arc::from("test.vn"),
        imports: vec![],
        exports: vec![],
        types,
        classes: vec![],
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
        },
    };
    m.top_level.body.push(TirStmt::Let {
        local: LocalId(0),
        ty: nullable_int,
        init: Some(expr(
            TirExprKind::StrLit("hello".into()),
            nullable_str,
            Resolution::None,
        )),
    });
    let errs = verify_module(&m).unwrap_err();
    assert!(
        errs.iter().any(|e| e.message.contains("declares")),
        "got: {:?}",
        errs
    );
}

#[test]
fn deeply_nested_nullable_chain_terminates_without_false_positive() {
    let mut types = TyTable::default();
    let mut id = types.intern(BackendTy::Int);
    for _ in 0..40 {
        id = types.intern(BackendTy::Nullable(id));
    }
    let deeply_nullable = BackendTy::Nullable(id);

    let mut m = TirModule {
        source_file: Arc::from("test.vn"),
        imports: vec![],
        exports: vec![],
        types,
        classes: vec![],
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
            locals: vec![deeply_nullable],
            body: vec![],
            has_this: false,
            this_class: None,
            is_async: false,
            is_generator: false,
            has_rest: false,
        },
    };

    m.top_level.body.push(TirStmt::Let {
        local: LocalId(0),
        ty: deeply_nullable,
        init: Some(expr(
            TirExprKind::IntLit(42),
            BackendTy::Int,
            Resolution::None,
        )),
    });

    let result = verify_module(&m);
    assert!(
        result.is_ok(),
        "deeply nested chain caused verification failure: {:?}",
        result
    );
}
