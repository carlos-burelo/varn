//! The counter that says whether the work is advancing, as opposed to the
//! verifier, which says whether it is broken. A receiver whose class IS known
//! but which resolves by name is legal — it is an opportunity lost, not a
//! miscompile — so it is counted, not rejected.

use std::sync::Arc;
use varn_tir::*;

fn module() -> TirModule {
    TirModule {
        source_file: Arc::from("test.vn"),
        imports: vec![],
        exports: vec![],
        types: TyTable::default(),
        classes: vec![ClassInfo::new(
            Arc::from("P"),
            None,
            vec![("x".into(), BackendTy::Int)],
        )],
        enums: vec![],
        signatures: vec![Signature {
            params: vec![],
            return_ty: BackendTy::Void,
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

/// Dynamics are counted per reason, not as one number. An honest host
/// boundary and an inference hole read identically in a total.
#[test]
fn dynamics_are_counted_by_reason() {
    let mut m = module();
    m.top_level.body.push(TirStmt::Expr(expr(
        TirExprKind::Var,
        BackendTy::Dynamic(DynReason::HostBoundary),
        Resolution::Local(LocalId(0)),
    )));
    m.top_level.body.push(TirStmt::Expr(expr(
        TirExprKind::Var,
        BackendTy::Dynamic(DynReason::Unannotated),
        Resolution::Local(LocalId(0)),
    )));

    let c = Coverage::of(&m);
    assert_eq!(c.dynamic_by_reason(DynReason::HostBoundary), 1);
    assert_eq!(c.dynamic_by_reason(DynReason::Unannotated), 1);
    assert_eq!(c.dynamic_by_reason(DynReason::Union), 0);
}

/// A known class resolved by name is legal and counted — that is the
/// difference between the verifier and this report.
#[test]
fn name_dispatch_on_a_known_class_is_counted_not_rejected() {
    let mut m = module();
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
        Resolution::ByName {
            name: "x".into(),
            why: DynReason::Unannotated,
        },
    )));

    assert!(
        verify_module(&m).is_ok(),
        "a lost opportunity is not an error"
    );
    let c = Coverage::of(&m);
    assert_eq!(c.name_dispatch, 1);
}

/// The ratio is what a regression gate compares across commits.
#[test]
fn the_static_ratio_is_reported() {
    let mut m = module();
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

    let c = Coverage::of(&m);
    assert!(c.static_ratio() > 0.0);
    assert!(c.report().contains("static"));
}
