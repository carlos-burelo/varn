use super::super::ir::{InstKind, SsaFunc, Value};
use vn_core::OpCode;
use vn_tir::BackendTy;

pub(super) struct Immediates {
    pub(super) imm: Vec<Option<i8>>,

    elided: Vec<bool>,
}

impl Immediates {
    pub(super) fn is_elided(&self, v: Value) -> bool {
        self.elided.get(v.0 as usize).copied().unwrap_or(false)
    }
}

pub(super) struct ImmediateForm {
    pub(super) carrier: Value,
    pub(super) other: Value,
    pub(super) value: i8,
    pub(super) opcode: OpCode,
}

pub(super) fn immediate_operand(kind: &InstKind, imm: &[Option<i8>]) -> Option<ImmediateForm> {
    let InstKind::Binary {
        op,
        lhs,
        rhs,
        ty: BackendTy::Int,
    } = kind
    else {
        return None;
    };
    let get = |v: &Value| -> Option<i8> { *imm.get(v.0 as usize)? };
    let form = |opcode: OpCode, swapped: bool| -> Option<ImmediateForm> {
        let (carrier, other) = if swapped { (lhs, rhs) } else { (rhs, lhs) };
        Some(ImmediateForm {
            carrier: *carrier,
            other: *other,
            value: get(carrier)?,
            opcode,
        })
    };
    use crate::hir::HirBinOp as H;
    match op {
        H::Add => form(OpCode::AddImm, false).or_else(|| form(OpCode::AddImm, true)),
        H::Sub => form(OpCode::SubImm, false),
        H::Lt => form(OpCode::LtImm, false).or_else(|| form(OpCode::GtImm, true)),
        H::Le => form(OpCode::LteImm, false).or_else(|| form(OpCode::GteImm, true)),
        H::Gt => form(OpCode::GtImm, false).or_else(|| form(OpCode::LtImm, true)),
        H::Ge => form(OpCode::GteImm, false).or_else(|| form(OpCode::LteImm, true)),
        H::Eq => form(OpCode::EqImm, false).or_else(|| form(OpCode::EqImm, true)),
        H::Ne => form(OpCode::NeqImm, false).or_else(|| form(OpCode::NeqImm, true)),
        H::Mul
        | H::Div
        | H::Mod
        | H::Pow
        | H::BitAnd
        | H::BitOr
        | H::BitXor
        | H::Shl
        | H::Shr
        | H::Ushr
        | H::Instanceof
        | H::In => None,
    }
}

pub(super) fn plan_immediates(ssa: &SsaFunc) -> Immediates {
    let n = ssa.values.len();
    let mut imm: Vec<Option<i8>> = vec![None; n];
    for block in &ssa.blocks {
        for inst in &block.insts {
            if let (Some(d), InstKind::ConstInt(i)) = (inst.dest, &inst.kind)
                && let Ok(small) = i8::try_from(*i)
            {
                imm[d.0 as usize] = Some(small);
            }
        }
    }

    let mut total = vec![0u32; n];
    let mut folded = vec![0u32; n];
    for block in &ssa.blocks {
        for inst in &block.insts {
            crate::ssa::uses::visit_uses(&inst.kind, &mut |v| total[v.0 as usize] += 1);
            if let Some(form) = immediate_operand(&inst.kind, &imm) {
                folded[form.carrier.0 as usize] += 1;
            }
        }
        crate::ssa::uses::visit_term_uses(&block.term, &mut |v| total[v.0 as usize] += 1);
    }

    let elided = (0..n)
        .map(|i| imm[i].is_some() && total[i] > 0 && total[i] == folded[i])
        .collect();
    Immediates { imm, elided }
}
