use super::common::{expr, int, module_with_point};
use std::sync::Arc;
use varn_tir::*;
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
            force_inline: false,
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
            force_inline: false,
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
            force_inline: false,
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
            force_inline: false,
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
            force_inline: false,
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
