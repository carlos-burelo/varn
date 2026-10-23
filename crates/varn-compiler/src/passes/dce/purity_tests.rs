use super::is_pure;
use crate::hir::{HirBinOp, HirUnOp};
use crate::ssa::ir::{InstKind, Value};
use varn_tir::BackendTy;

fn bin(op: HirBinOp, ty: BackendTy) -> InstKind {
    InstKind::Binary {
        op,
        lhs: Value(0),
        rhs: Value(1),
        ty,
    }
}

#[test]
fn checked_int_arithmetic_is_not_pure() {
    for op in [HirBinOp::Add, HirBinOp::Sub, HirBinOp::Mul] {
        assert!(
            !is_pure(&bin(op, BackendTy::Int)),
            "{op:?} on int can overflow"
        );
    }
    assert!(!is_pure(&InstKind::Unary {
        op: HirUnOp::Neg,
        operand: Value(0),
        ty: BackendTy::Int,
    }));
}

#[test]
fn float_arithmetic_and_int_comparisons_stay_pure() {
    for op in [HirBinOp::Add, HirBinOp::Sub, HirBinOp::Mul] {
        assert!(is_pure(&bin(op, BackendTy::Float)));
    }
    assert!(is_pure(&bin(HirBinOp::Lt, BackendTy::Int)));
    assert!(is_pure(&bin(HirBinOp::BitAnd, BackendTy::Int)));
}
