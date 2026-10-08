use super::super::ir::{InstKind, SsaFunc, Value};

pub(super) struct Immediates {
    pub(super) imm: Vec<Option<i8>>,

    elided: Vec<bool>,
}

impl Immediates {
    pub(super) fn is_elided(&self, v: Value) -> bool {
        self.elided.get(v.0 as usize).copied().unwrap_or(false)
    }
}

pub(super) fn immediate_operand(kind: &InstKind, imm: &[Option<i8>]) -> Option<(Value, Value, i8)> {
    let InstKind::Binary {
        op,
        lhs,
        rhs,
        ty: crate::hir::HirType::Int,
    } = kind
    else {
        return None;
    };
    let get = |v: &Value| -> Option<i8> { *imm.get(v.0 as usize)? };
    match op {
        crate::hir::HirBinOp::Add => {
            if let Some(i) = get(rhs) {
                Some((*rhs, *lhs, i))
            } else {
                get(lhs).map(|i| (*lhs, *rhs, i))
            }
        }
        crate::hir::HirBinOp::Sub => get(rhs).map(|i| (*rhs, *lhs, i)),
        crate::hir::HirBinOp::Mul
        | crate::hir::HirBinOp::Div
        | crate::hir::HirBinOp::Mod
        | crate::hir::HirBinOp::Pow
        | crate::hir::HirBinOp::Eq
        | crate::hir::HirBinOp::Ne
        | crate::hir::HirBinOp::Lt
        | crate::hir::HirBinOp::Le
        | crate::hir::HirBinOp::Gt
        | crate::hir::HirBinOp::Ge
        | crate::hir::HirBinOp::BitAnd
        | crate::hir::HirBinOp::BitOr
        | crate::hir::HirBinOp::BitXor
        | crate::hir::HirBinOp::Shl
        | crate::hir::HirBinOp::Shr
        | crate::hir::HirBinOp::Ushr
        | crate::hir::HirBinOp::Instanceof
        | crate::hir::HirBinOp::In => None,
    }
}

pub(super) fn plan_immediates(ssa: &SsaFunc) -> Immediates {
    let n = ssa.values.len();
    let mut imm: Vec<Option<i8>> = vec![None; n];
    for block in &ssa.blocks {
        for inst in &block.insts {
            if let (Some(d), InstKind::ConstInt(i)) = (inst.dest, &inst.kind) {
                if let Ok(small) = i8::try_from(*i) {
                    imm[d.0 as usize] = Some(small);
                }
            }
        }
    }

    let mut total = vec![0u32; n];
    let mut folded = vec![0u32; n];
    for block in &ssa.blocks {
        for inst in &block.insts {
            crate::ssa::uses::visit_uses(&inst.kind, &mut |v| total[v.0 as usize] += 1);
            if let Some((carrier, _, _)) = immediate_operand(&inst.kind, &imm) {
                folded[carrier.0 as usize] += 1;
            }
        }
        crate::ssa::uses::visit_term_uses(&block.term, &mut |v| total[v.0 as usize] += 1);
    }

    let elided = (0..n)
        .map(|i| imm[i].is_some() && total[i] > 0 && total[i] == folded[i])
        .collect();
    Immediates { imm, elided }
}
