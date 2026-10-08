use super::common::{expr, int};
use std::sync::Arc;
use varn_tir::*;
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
            force_inline: false,
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
            force_inline: false,
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
            force_inline: false,
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
            force_inline: false,
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
            force_inline: false,
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
