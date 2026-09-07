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
        global_names: vec![],
        top_level: TirFunction {
            name: Rc::from("<module>"),
            sig: SigId(0),
            params: vec![],
            return_ty: BackendTy::Void,
            locals: vec![BackendTy::Class(ClassId(0))],
            body: vec![],
            has_this: false,
            this_class: None,
            is_async: false,
            is_generator: false,
        },
    }
}

fn expr(kind: TirExprKind, ty: BackendTy, res: Resolution) -> TirExpr {
    TirExpr { kind, ty, res, span: Span::EMPTY }
}

fn int(v: i64) -> TirExpr {
    expr(TirExprKind::IntLit(v), BackendTy::Int, Resolution::None)
}

/// `return null` in a `T?` function: bare null (Nullable over Never) is
/// assignable to every nullable type.
#[test]
fn bare_null_returns_from_any_nullable_function() {
    let mut m = module_with_point();
    let never_id = m.types.intern(BackendTy::Never);
    let int_id = m.types.intern(BackendTy::Int);
    m.signatures.push(Signature {
        params: vec![],
        return_ty: BackendTy::Nullable(int_id),
    });
    m.functions.push(TirFunction {
        name: Rc::from("first"),
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
    });
    assert!(verify_module(&m).is_ok(), "{:?}", verify_module(&m));
}

/// `int` widens to `float` at a call argument — `takesFloat(1)`.
#[test]
fn int_argument_widens_to_a_float_parameter() {
    let mut m = module_with_point();
    m.signatures.push(Signature {
        params: vec![BackendTy::Float],
        return_ty: BackendTy::Void,
    });
    m.functions.push(TirFunction {
        name: Rc::from("takesFloat"),
        sig: SigId(1),
        params: vec![BackendTy::Float],
        return_ty: BackendTy::Void,
        locals: vec![],
        body: vec![],
        has_this: false,
        this_class: None,
        is_async: false,
        is_generator: false,
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

/// A subclass argument satisfies an ancestor parameter.
#[test]
fn a_subclass_is_assignable_to_its_parent() {
    // ClassId(0) = Animal, ClassId(1) = Dog extends Animal.
    let animal = ClassInfo::new(Rc::from("Animal"), None, vec![]);
    let mut dog = ClassInfo::new(Rc::from("Dog"), None, vec![]);
    dog.parent = Some(ClassId(0));
    let mut m = module_with_point();
    m.classes = vec![animal, dog];
    m.signatures.push(Signature {
        params: vec![BackendTy::Class(ClassId(0))],
        return_ty: BackendTy::Void,
    });
    m.functions.push(TirFunction {
        name: Rc::from("greet"),
        sig: SigId(1),
        params: vec![BackendTy::Class(ClassId(0))],
        return_ty: BackendTy::Void,
        locals: vec![BackendTy::Class(ClassId(1))],
        body: vec![],
        has_this: false,
        this_class: None,
        is_async: false,
        is_generator: false,
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
        global_names: vec![],
        top_level: TirFunction {
            name: Rc::from("<module>"),
            sig: SigId(1),
            params: vec![],
            return_ty: BackendTy::Void,
            locals: vec![BackendTy::Class(ClassId(0))],
            body: vec![],
            has_this: false,
            this_class: None,
            is_async: false,
            is_generator: false,
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
        global_names: vec![],
        top_level: TirFunction {
            name: Rc::from("<module>"),
            sig: SigId(1),
            params: vec![],
            return_ty: BackendTy::Void,
            locals: vec![BackendTy::Class(ClassId(0))],
            body: vec![],
            has_this: false,
            this_class: None,
            is_async: false,
            is_generator: false,
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
    m.top_level.locals.push(BackendTy::Int);
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
        global_names: vec![],
        top_level: TirFunction {
            name: Rc::from("<module>"),
            sig: SigId(0),
            params: vec![],
            return_ty: BackendTy::Int,
            locals: vec![],
            body: vec![],
            has_this: false,
            this_class: None,
            is_async: false,
            is_generator: false,
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
        global_names: vec![],
        top_level: TirFunction {
            name: Rc::from("<module>"),
            sig: SigId(0),
            params: vec![],
            return_ty: BackendTy::Int,
            locals: vec![],
            body: vec![],
            has_this: false,
            this_class: None,
            is_async: false,
            is_generator: false,
        },
    };
    m.top_level.body.push(TirStmt::Return(None));
    let errs = verify_module(&m).unwrap_err();
    assert!(errs.iter().any(|e| e.message.contains("return type")), "got: {:?}", errs);
}

/// A let binding declared Nullable(T) initialized with T is valid (assignability).
#[test]
fn let_with_nullable_declared_and_nonnull_init_is_valid() {
    let mut types = TyTable::default();
    let int_id = types.intern(BackendTy::Int);
    let nullable_int = BackendTy::Nullable(int_id);
    let mut m = TirModule {
        source_file: Rc::from("test.vn"),
        types,
        classes: vec![],
        enums: vec![],
        signatures: vec![Signature { params: vec![], return_ty: BackendTy::Void }],
        functions: vec![],
        globals: vec![],
        global_names: vec![],
        top_level: TirFunction {
            name: Rc::from("<module>"),
            sig: SigId(0),
            params: vec![],
            return_ty: BackendTy::Void,
            locals: vec![nullable_int],
            body: vec![],
            has_this: false,
            this_class: None,
            is_async: false,
            is_generator: false,
        },
    };
    m.top_level.body.push(TirStmt::Let {
        local: LocalId(0),
        ty: nullable_int,
        init: Some(int(42)), // non-null Int goes into Nullable(Int)
    });
    assert!(verify_module(&m).is_ok());
}

/// A let binding declared T initialized with Nullable(T) is rejected (needs narrowing).
#[test]
fn let_with_nonnull_declared_and_nullable_init_is_rejected() {
    let mut types = TyTable::default();
    let int_id = types.intern(BackendTy::Int);
    let nullable_int = BackendTy::Nullable(int_id);
    let mut m = TirModule {
        source_file: Rc::from("test.vn"),
        types,
        classes: vec![],
        enums: vec![],
        signatures: vec![Signature { params: vec![], return_ty: BackendTy::Void }],
        functions: vec![],
        globals: vec![],
        global_names: vec![],
        top_level: TirFunction {
            name: Rc::from("<module>"),
            sig: SigId(0),
            params: vec![],
            return_ty: BackendTy::Void,
            locals: vec![],
            body: vec![],
            has_this: false,
            this_class: None,
            is_async: false,
            is_generator: false,
        },
    };
    m.top_level.body.push(TirStmt::Let {
        local: LocalId(0),
        ty: BackendTy::Int, // declared Int
        init: Some(expr(
            TirExprKind::IntLit(42),
            nullable_int, // but init is Nullable(Int)
            Resolution::None,
        )),
    });
    let errs = verify_module(&m).unwrap_err();
    assert!(errs.iter().any(|e| e.message.contains("declares")), "got: {:?}", errs);
}

/// A return of Never is valid in any function (Never inhabits every type).
#[test]
fn return_of_never_type_is_valid_anywhere() {
    let mut m = TirModule {
        source_file: Rc::from("test.vn"),
        types: TyTable::default(),
        classes: vec![],
        enums: vec![],
        signatures: vec![Signature { params: vec![], return_ty: BackendTy::Int }],
        functions: vec![],
        globals: vec![],
        global_names: vec![],
        top_level: TirFunction {
            name: Rc::from("<module>"),
            sig: SigId(0),
            params: vec![],
            return_ty: BackendTy::Int,
            locals: vec![],
            body: vec![],
            has_this: false,
            this_class: None,
            is_async: false,
            is_generator: false,
        },
    };
    m.top_level.body.push(TirStmt::Return(Some(expr(
        TirExprKind::IntLit(0),
        BackendTy::Never, // Never inhabits Int
        Resolution::None,
    ))));
    assert!(verify_module(&m).is_ok());
}

/// A return declared Nullable(T) with value T is valid (assignability).
#[test]
fn return_nonnull_when_function_returns_nullable_is_valid() {
    let mut types = TyTable::default();
    let int_id = types.intern(BackendTy::Int);
    let mut m = TirModule {
        source_file: Rc::from("test.vn"),
        types,
        classes: vec![],
        enums: vec![],
        signatures: vec![Signature { params: vec![], return_ty: BackendTy::Nullable(int_id) }],
        functions: vec![],
        globals: vec![],
        global_names: vec![],
        top_level: TirFunction {
            name: Rc::from("<module>"),
            sig: SigId(0),
            params: vec![],
            return_ty: BackendTy::Nullable(int_id), // function returns Int?
            locals: vec![],
            body: vec![],
            has_this: false,
            this_class: None,
            is_async: false,
            is_generator: false,
        },
    };
    m.top_level.body.push(TirStmt::Return(Some(int(42)))); // returning Int
    assert!(verify_module(&m).is_ok());
}

/// A return declared T with value Nullable(T) is rejected (needs narrowing).
#[test]
fn return_nullable_when_function_returns_nonnull_is_rejected() {
    let mut types = TyTable::default();
    let int_id = types.intern(BackendTy::Int);
    let mut m = TirModule {
        source_file: Rc::from("test.vn"),
        types,
        classes: vec![],
        enums: vec![],
        signatures: vec![Signature { params: vec![], return_ty: BackendTy::Int }],
        functions: vec![],
        globals: vec![],
        global_names: vec![],
        top_level: TirFunction {
            name: Rc::from("<module>"),
            sig: SigId(0),
            params: vec![],
            return_ty: BackendTy::Int, // function returns Int
            locals: vec![],
            body: vec![],
            has_this: false,
            this_class: None,
            is_async: false,
            is_generator: false,
        },
    };
    m.top_level.body.push(TirStmt::Return(Some(expr(
        TirExprKind::IntLit(42),
        BackendTy::Nullable(int_id), // returning Int?
        Resolution::None,
    ))));
    let errs = verify_module(&m).unwrap_err();
    assert!(errs.iter().any(|e| e.message.contains("return type")), "got: {:?}", errs);
}

/// A method call with argument T where parameter is Nullable(T) passes.
#[test]
fn method_call_arg_nonnull_into_nullable_param_passes() {
    let mut types = TyTable::default();
    let int_id = types.intern(BackendTy::Int);
    let mut m = TirModule {
        source_file: Rc::from("test.vn"),
        types,
        classes: vec![ClassInfo::new_with_methods(
            Rc::from("Point"),
            None,
            vec![],
            vec![("move".into(), SigId(0))],
        )],
        enums: vec![],
        signatures: vec![
            Signature {
                params: vec![BackendTy::Nullable(int_id)],
                return_ty: BackendTy::Void,
            },
            Signature { params: vec![], return_ty: BackendTy::Void },
        ],
        functions: vec![],
        globals: vec![],
        global_names: vec![],
        top_level: TirFunction {
            name: Rc::from("<module>"),
            sig: SigId(1),
            params: vec![],
            return_ty: BackendTy::Void,
            locals: vec![BackendTy::Class(ClassId(0))],
            body: vec![],
            has_this: false,
            this_class: None,
            is_async: false,
            is_generator: false,
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
            args: vec![TirArg::Expr(int(42))], // Int goes into Nullable(Int)
        },
        BackendTy::Void,
        Resolution::VtableSlot(0),
    )));
    assert!(verify_module(&m).is_ok());
}

/// A method call with argument Nullable(T) where parameter is T is rejected.
#[test]
fn method_call_arg_nullable_into_nonnull_param_rejected() {
    let mut types = TyTable::default();
    let int_id = types.intern(BackendTy::Int);
    let mut m = TirModule {
        source_file: Rc::from("test.vn"),
        types,
        classes: vec![ClassInfo::new_with_methods(
            Rc::from("Point"),
            None,
            vec![],
            vec![("move".into(), SigId(0))],
        )],
        enums: vec![],
        signatures: vec![
            Signature {
                params: vec![BackendTy::Int],
                return_ty: BackendTy::Void,
            },
            Signature { params: vec![], return_ty: BackendTy::Void },
        ],
        functions: vec![],
        globals: vec![],
        global_names: vec![],
        top_level: TirFunction {
            name: Rc::from("<module>"),
            sig: SigId(1),
            params: vec![],
            return_ty: BackendTy::Void,
            locals: vec![BackendTy::Class(ClassId(0))],
            body: vec![],
            has_this: false,
            this_class: None,
            is_async: false,
            is_generator: false,
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
                BackendTy::Nullable(int_id), // Nullable(Int) goes into Int
                Resolution::None,
            ))],
        },
        BackendTy::Void,
        Resolution::VtableSlot(0),
    )));
    let errs = verify_module(&m).unwrap_err();
    assert!(errs.iter().any(|e| e.message.contains("argument")), "got: {:?}", errs);
}

/// PERMISSIVENESS CHECK: Nullable(Int) must NOT accept Str.
#[test]
fn let_declared_nullable_int_init_str_should_fail() {
    let mut types = TyTable::default();
    let int_id = types.intern(BackendTy::Int);
    let nullable_int = BackendTy::Nullable(int_id);
    let mut m = TirModule {
        source_file: Rc::from("test.vn"),
        types,
        classes: vec![],
        enums: vec![],
        signatures: vec![Signature { params: vec![], return_ty: BackendTy::Void }],
        functions: vec![],
        globals: vec![],
        global_names: vec![],
        top_level: TirFunction {
            name: Rc::from("<module>"),
            sig: SigId(0),
            params: vec![],
            return_ty: BackendTy::Void,
            locals: vec![],
            body: vec![],
            has_this: false,
            this_class: None,
            is_async: false,
            is_generator: false,
        },
    };
    m.top_level.body.push(TirStmt::Let {
        local: LocalId(0),
        ty: nullable_int,
        init: Some(expr(
            TirExprKind::StrLit("hello".into()),
            BackendTy::Str, // Str into Nullable(Int) should fail
            Resolution::None,
        )),
    });
    let errs = verify_module(&m).unwrap_err();
    assert!(errs.iter().any(|e| e.message.contains("declares")), "got: {:?}", errs);
}

/// PERMISSIVENESS CHECK: Nullable(Int) must NOT accept Nullable(Str).
#[test]
fn let_declared_nullable_int_init_nullable_str_should_fail() {
    let mut types = TyTable::default();
    let int_id = types.intern(BackendTy::Int);
    let str_id = types.intern(BackendTy::Str);
    let nullable_int = BackendTy::Nullable(int_id);
    let nullable_str = BackendTy::Nullable(str_id);
    let mut m = TirModule {
        source_file: Rc::from("test.vn"),
        types,
        classes: vec![],
        enums: vec![],
        signatures: vec![Signature { params: vec![], return_ty: BackendTy::Void }],
        functions: vec![],
        globals: vec![],
        global_names: vec![],
        top_level: TirFunction {
            name: Rc::from("<module>"),
            sig: SigId(0),
            params: vec![],
            return_ty: BackendTy::Void,
            locals: vec![],
            body: vec![],
            has_this: false,
            this_class: None,
            is_async: false,
            is_generator: false,
        },
    };
    m.top_level.body.push(TirStmt::Let {
        local: LocalId(0),
        ty: nullable_int,
        init: Some(expr(
            TirExprKind::StrLit("hello".into()),
            nullable_str, // Nullable(Str) into Nullable(Int) should fail
            Resolution::None,
        )),
    });
    let errs = verify_module(&m).unwrap_err();
    assert!(errs.iter().any(|e| e.message.contains("declares")), "got: {:?}", errs);
}

/// A deeply nested chain of `Nullable` terminates and is not falsely
/// reported.
///
/// `TyTable` is append-only (see its doc comment in `ty.rs`): `intern`
/// assigns an index only after pushing, so any entry can reference only
/// strictly smaller `TyId`s. The type graph is therefore a DAG, and a real
/// cycle is not constructible through the public API — this test does NOT
/// build one. What it builds is a straight-line chain of `Nullable` wrapping
/// `Nullable` wrapping ... `Int`, deep enough (deeper than the 32-level depth
/// bound in `assignable` and `non_nullable`) to actually cross that bound
/// rather than terminate on its own before reaching it. That exercises the
/// depth-bound path itself, not just ordinary recursion: it asserts
/// `verify_module` returns rather than hanging, and that saturating the
/// bound on a type that is merely deep — not cyclic — does not produce a
/// false positive.
#[test]
fn deeply_nested_nullable_chain_terminates_without_false_positive() {
    // Build a chain of 40 Nullable levels, each interned from the previous,
    // strictly increasing TyId by construction. 40 > the 32-level bound.
    let mut types = TyTable::default();
    let mut id = types.intern(BackendTy::Int);
    for _ in 0..40 {
        id = types.intern(BackendTy::Nullable(id));
    }
    let deeply_nullable = BackendTy::Nullable(id);

    let mut m = TirModule {
        source_file: Rc::from("test.vn"),
        types,
        classes: vec![],
        enums: vec![],
        signatures: vec![Signature { params: vec![], return_ty: BackendTy::Void }],
        functions: vec![],
        globals: vec![],
        global_names: vec![],
        top_level: TirFunction {
            name: Rc::from("<module>"),
            sig: SigId(0),
            params: vec![],
            return_ty: BackendTy::Void,
            locals: vec![deeply_nullable],
            body: vec![],
            has_this: false,
            this_class: None,
            is_async: false,
            is_generator: false,
        },
    };

    // A non-null Int is assignable to any depth of Nullable(...Nullable(Int)).
    m.top_level.body.push(TirStmt::Let {
        local: LocalId(0),
        ty: deeply_nullable,
        init: Some(expr(TirExprKind::IntLit(42), BackendTy::Int, Resolution::None)),
    });

    // The key assertion: verify_module must RETURN, not hang, and must not
    // falsely reject a merely-deep (non-cyclic) chain once the bound
    // saturates.
    let result = verify_module(&m);
    assert!(result.is_ok(), "deeply nested chain caused verification failure: {:?}", result);
}
