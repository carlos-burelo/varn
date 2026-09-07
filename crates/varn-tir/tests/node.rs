//! The type is a field of the NODE, not of some variants of an enum.
//!
//! In HirExpr, `ty` sits on Binary and Member but not on Array, Object,
//! Assign, OptionalChain or TryOp — for those there is nowhere to write a
//! type, so the consumer assumes one. A struct with a mandatory field cannot
//! have that shape.

use varn_tir::{BackendTy, Resolution, Span, TirArrayEl, TirExpr, TirExprKind};

/// Every node has a type, whatever its kind. This is a compile-time property
/// — the test exists to pin it, because the moment `ty` becomes an Option the
/// old failure mode is back.
#[test]
fn every_node_kind_carries_a_type() {
    let lit = TirExpr {
        kind: TirExprKind::IntLit(42),
        ty: BackendTy::Int,
        res: Resolution::None,
        span: Span::EMPTY,
    };
    assert_eq!(lit.ty, BackendTy::Int);

    // An array literal — one of the kinds HIR had no slot for.
    let arr = TirExpr {
        kind: TirExprKind::ArrayLit(vec![TirArrayEl::Expr(lit)]),
        ty: BackendTy::Int, // stands in for Array(TyId) built from a real table
        res: Resolution::None,
        span: Span::EMPTY,
    };
    assert_eq!(arr.ty, BackendTy::Int);
}

/// A field access is one kind with a resolution, not two separate variants.
/// The Member / GetFixedField split in HIR is what propagates into the
/// GetProperty / GetFixedField opcode pair and every consumer below it.
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
    // Same kind. Only the resolution differs.
    assert!(matches!(fast.kind, TirExprKind::Field { .. }));
    assert!(matches!(slow.kind, TirExprKind::Field { .. }));
}
