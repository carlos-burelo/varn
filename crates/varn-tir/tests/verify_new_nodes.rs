//! The nodes added as the entry condition of stage 2: `await` / `yield`,
//! `IsNull`, the enum discriminant and payload accessors, `TypeTest`, and
//! spread in argument and element lists. Each test builds malformed TIR by
//! hand and pins that the verifier rejects it — the only instrument that
//! works before there is a language to run.

use std::rc::Rc;
use varn_tir::*;

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

fn func(name: &str, is_async: bool, is_generator: bool, body: Vec<TirStmt>) -> TirFunction {
    TirFunction {
        name: Rc::from(name),
        sig: SigId(0),
        params: vec![],
        return_ty: BackendTy::Void,
        locals: vec![],
        body,
        has_this: false,
        this_class: None,
        is_async,
        is_generator,
        has_rest: false,
    }
}

fn module(top_level: TirFunction) -> TirModule {
    TirModule {
        source_file: Rc::from("test.vn"),
        imports: vec![],
        exports: vec![],
        types: TyTable::default(),
        classes: vec![],
        enums: vec![],
        signatures: vec![Signature {
            params: vec![],
            return_ty: BackendTy::Void,
        }],
        functions: vec![],
        globals: vec![],
        global_names: vec![],
        class_defs: vec![],
        top_level,
    }
}

fn errors_of(m: &TirModule) -> Vec<VerifyError> {
    verify_module(m).unwrap_err()
}

#[test]
fn await_in_a_non_async_function_is_rejected() {
    let body = vec![TirStmt::Expr(expr(
        TirExprKind::Await {
            future: Box::new(int(1)),
        },
        BackendTy::Int,
        Resolution::None,
    ))];
    let m = module(func("<module>", false, false, body));
    assert!(errors_of(&m).iter().any(|e| e.message.contains("await")));
}

#[test]
fn await_in_an_async_function_is_fine() {
    let body = vec![TirStmt::Expr(expr(
        TirExprKind::Await {
            future: Box::new(int(1)),
        },
        BackendTy::Int,
        Resolution::None,
    ))];
    let m = module(func("<module>", true, false, body));
    assert!(verify_module(&m).is_ok());
}

#[test]
fn yield_in_a_non_generator_is_rejected() {
    let body = vec![TirStmt::Expr(expr(
        TirExprKind::Yield {
            value: Some(Box::new(int(1))),
            delegate: false,
        },
        BackendTy::Dynamic(DynReason::NotYetSupported),
        Resolution::None,
    ))];
    let m = module(func("<module>", false, false, body));
    assert!(errors_of(&m).iter().any(|e| e.message.contains("yield")));
}

#[test]
fn is_null_must_produce_bool() {
    let body = vec![TirStmt::Expr(expr(
        TirExprKind::Unary {
            op: TirUnOp::IsNull,
            operand: Box::new(int(1)),
        },
        BackendTy::Int, // wrong: an IsNull is a Bool
        Resolution::None,
    ))];
    let m = module(func("<module>", false, false, body));
    assert!(errors_of(&m).iter().any(|e| e.message.contains("IsNull")));
}

#[test]
fn discriminant_of_a_non_enum_is_rejected() {
    let body = vec![TirStmt::Expr(expr(
        TirExprKind::Discriminant {
            value: Box::new(int(1)),
        },
        BackendTy::Int,
        Resolution::None,
    ))];
    let m = module(func("<module>", false, false, body));
    assert!(errors_of(&m)
        .iter()
        .any(|e| e.message.contains("not an enum")));
}

fn module_with_enum(top_level: TirFunction) -> TirModule {
    let mut types = TyTable::default();
    let _ = types.intern(BackendTy::Int);
    TirModule {
        source_file: Rc::from("test.vn"),
        imports: vec![],
        exports: vec![],
        types,
        classes: vec![],
        enums: vec![EnumInfo {
            name: Rc::from("Shape"),
            variants: vec![VariantInfo {
                name: Rc::from("Circle"),
                tag: 0,
                payload: vec![BackendTy::Int],
            }],
        }],
        signatures: vec![Signature {
            params: vec![],
            return_ty: BackendTy::Void,
        }],
        functions: vec![],
        globals: vec![],
        global_names: vec![],
        class_defs: vec![],
        top_level,
    }
}

#[test]
fn variant_payload_with_the_wrong_type_is_rejected() {
    let recv = expr(
        TirExprKind::Var,
        BackendTy::Enum(EnumId(0)),
        Resolution::Local(LocalId(0)),
    );
    let mut top = func("<module>", false, false, vec![]);
    top.locals = vec![BackendTy::Enum(EnumId(0))];
    top.body = vec![TirStmt::Expr(expr(
        TirExprKind::VariantPayload {
            value: Box::new(recv),
            tag: 0,
            field: 0,
        },
        BackendTy::Str, // the Circle payload field is Int
        Resolution::EnumVariant {
            enum_id: EnumId(0),
            tag: 0,
        },
    ))];
    let m = module_with_enum(top);
    assert!(errors_of(&m).iter().any(|e| e.message.contains("field 0")));
}

#[test]
fn variant_payload_out_of_range_field_is_rejected() {
    let recv = expr(
        TirExprKind::Var,
        BackendTy::Enum(EnumId(0)),
        Resolution::Local(LocalId(0)),
    );
    let mut top = func("<module>", false, false, vec![]);
    top.locals = vec![BackendTy::Enum(EnumId(0))];
    top.body = vec![TirStmt::Expr(expr(
        TirExprKind::VariantPayload {
            value: Box::new(recv),
            tag: 0,
            field: 7,
        },
        BackendTy::Int,
        Resolution::EnumVariant {
            enum_id: EnumId(0),
            tag: 0,
        },
    ))];
    let m = module_with_enum(top);
    assert!(errors_of(&m)
        .iter()
        .any(|e| e.message.contains("out of range")));
}

#[test]
fn type_test_naming_a_missing_class_is_rejected() {
    let body = vec![TirStmt::Expr(expr(
        TirExprKind::TypeTest {
            value: Box::new(int(1)),
            class: ClassId(9),
        },
        BackendTy::Bool,
        Resolution::None,
    ))];
    let m = module(func("<module>", false, false, body));
    assert!(errors_of(&m)
        .iter()
        .any(|e| e.message.contains("TypeTest names ClassId(9)")));
}

#[test]
fn a_spread_argument_suppresses_the_arity_check() {
    // fn takes 0 params (SigId 0); the call passes a spread, so no arity error.
    let mut top = func("<module>", false, false, vec![]);
    top.body = vec![TirStmt::Expr(expr(
        TirExprKind::Call {
            callee: Box::new(expr(TirExprKind::Var, BackendTy::Void, Resolution::None)),
            args: vec![TirArg::Spread(int(1))],
        },
        BackendTy::Void,
        Resolution::DirectFn(FnId(0)),
    ))];
    let mut m = module(top);
    m.functions = vec![func("f", false, false, vec![])];
    // FnId(0)'s signature is SigId(0), arity 0; a positional [x] would error,
    // but a spread must not.
    assert!(verify_module(&m).is_ok(), "{:?}", verify_module(&m));
}
