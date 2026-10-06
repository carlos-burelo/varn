#![allow(unused_crate_dependencies)]







use varn_tir::{BackendTy, Resolution, Span, TirArrayEl, TirExpr, TirExprKind};




#[test]
fn every_node_kind_carries_a_type() {
    let lit = TirExpr {
        kind: TirExprKind::IntLit(42),
        ty: BackendTy::Int,
        res: Resolution::None,
        span: Span::EMPTY,
    };
    assert_eq!(lit.ty, BackendTy::Int);

    
    let arr = TirExpr {
        kind: TirExprKind::ArrayLit(vec![TirArrayEl::Expr(lit)]),
        ty: BackendTy::Int, 
        res: Resolution::None,
        span: Span::EMPTY,
    };
    assert_eq!(arr.ty, BackendTy::Int);
}




#[test]
fn field_access_is_one_kind_with_a_resolution() {
    let recv = TirExpr {
        kind: TirExprKind::IntLit(0),
        ty: BackendTy::Int,
        res: Resolution::None,
        span: Span::EMPTY,
    };
    let fast = TirExpr {
        kind: TirExprKind::Field {
            object: Box::new(recv.clone()),
            name: "x".into(),
        },
        ty: BackendTy::Int,
        res: Resolution::FieldSlot(0),
        span: Span::EMPTY,
    };
    let slow = TirExpr {
        kind: TirExprKind::Field {
            object: Box::new(recv),
            name: "x".into(),
        },
        ty: BackendTy::Int,
        res: Resolution::ByName {
            name: "x".into(),
            why: varn_tir::DynReason::IndexSignature,
        },
        span: Span::EMPTY,
    };
    assert!(fast.res.is_static_dispatch());
    assert!(!slow.res.is_static_dispatch());
    
    assert!(matches!(fast.kind, TirExprKind::Field { .. }));
    assert!(matches!(slow.kind, TirExprKind::Field { .. }));
}
