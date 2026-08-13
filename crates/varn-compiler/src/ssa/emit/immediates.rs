//! Which `ConstInt` values ride inside an arithmetic opcode instead of taking a
//! register of their own.

use super::super::ir::{InstKind, SsaFunc, Value};


/// Which `ConstInt`s can ride along inside an arithmetic opcode.
///
/// `AddImm`/`SubImm` carry a signed 8-bit operand, so `i + 1` needs no
/// register for the `1` and no `LoadInt` to put it there. Both opcodes were
/// fully supported by the VM, the Cranelift lowering, the disassembler and
/// register allocation, and emitted by nothing.
pub(super) struct Immediates {
    /// The immediate a value can be folded to, if it is a small `ConstInt`.
    pub(super) imm: Vec<Option<i8>>,
    /// Values whose every use folds, so their `LoadInt` is never emitted.
    elided: Vec<bool>,
}

impl Immediates {
    pub(super) fn is_elided(&self, v: Value) -> bool {
        self.elided.get(v.0 as usize).copied().unwrap_or(false)
    }
}

/// An `Add`/`Sub` on proven ints is the only place an immediate can land.
/// `Add` is commutative so either side may carry it; `Sub` only the right,
/// since there is no reverse-subtract opcode.
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
        _ => None,
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

    // A constant only disappears if *every* read of it folds. `c + c` reads
    // the same value twice and can only fold one side, so it keeps its
    // register — hence counting rather than a boolean.
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

