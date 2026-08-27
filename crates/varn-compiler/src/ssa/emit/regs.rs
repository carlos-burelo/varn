//! SSA value -> machine register assignment, and the fixed frame layout the
//! rest of emission assumes (params first, then locals, then temporaries).

use super::super::ir::{InstKind, SsaFunc, VarId};
use super::slot_kind_of;
use crate::OptError;

type Result<T> = std::result::Result<T, OptError>;

pub(super) fn assign_registers(
    ssa: &SsaFunc,
    nparams: usize,
) -> Result<(Vec<u8>, u8, u8, u8, u16)> {
    let mut reg = vec![0u8; ssa.values.len()];
    let mut assigned = vec![false; ssa.values.len()];

    let entry_params = &ssa.blocks[ssa.entry.0 as usize].params;
    if entry_params.len() != nparams {
        return Err(OptError::Unsupported("ssa-emit: entry params != arity"));
    }
    for (i, p) in entry_params.iter().enumerate() {
        reg[p.0 as usize] = (1 + i) as u8;
        assigned[p.0 as usize] = true;
    }

    let nvals = ssa.values.len();
    let order = super::emission_order(ssa);
    let lv = crate::ssa::liveness::Liveness::analyze_ordered(ssa, &order);
    let def = lv.def;
    let end = lv.end;

    let mut base = 1 + nparams as u32;
    for block in &ssa.blocks {
        for inst in &block.insts {
            if let InstKind::LoadCaptured { var } | InstKind::StoreCaptured { var, .. } = &inst.kind
            {
                base = base.max(var_reg(*var, nparams) as u32 + 1);
            }
        }
    }
    let mut order: Vec<usize> = (0..nvals)
        .filter(|&v| !assigned[v] && def[v] != u32::MAX)
        .collect();
    order.sort_by_key(|&v| def[v]);
    // Type-aware linear scan: float values and non-float values draw from
    // separate free-register pools so no physical register is ever shared
    // between a float and a non-float value. Packing the two into one register
    // (their live ranges are disjoint, so a type-agnostic allocator would)
    // makes `derive_register_meta` meet their kinds to Dynamic, which erases
    // the float type the backend needs to route native f64. A pure-int
    // function never fills the float pool, so its allocation is unchanged.
    let is_float_v = |v: usize| {
        matches!(
            slot_kind_of(ssa.values[v].ty),
            varn_types::register_meta::SlotKind::Float
        )
    };
    let mut next: u32 = base;
    let mut free_float: Vec<u32> = Vec::new();
    let mut free_other: Vec<u32> = Vec::new();
    // (interval end, register, is_float)
    let mut active: Vec<(u32, u32, bool)> = Vec::new();
    for &v in &order {
        let d = def[v];
        let vf = is_float_v(v);
        let mut i = 0;
        while i < active.len() {
            if active[i].0 < d {
                let (_, r, rf) = active[i];
                if rf {
                    free_float.push(r);
                } else {
                    free_other.push(r);
                }
                active.swap_remove(i);
            } else {
                i += 1;
            }
        }
        let pool = if vf { &mut free_float } else { &mut free_other };
        let r = match pool.iter().copied().min() {
            Some(m) => {
                pool.retain(|&x| x != m);
                m
            }
            None => {
                let r = next;
                next += 1;
                r
            }
        };
        if r > 255 {
            return Err(OptError::Unsupported(
                "ssa-emit: register count exceeds 255",
            ));
        }
        reg[v] = r as u8;
        assigned[v] = true;
        active.push((end[v], r, vf));
    }

    let mut max_call = 0u32;
    for block in &ssa.blocks {
        for inst in &block.insts {
            let t = match &inst.kind {
                InstKind::Call { args, .. } => args.len() as u32 + 1,
                InstKind::SelfCall { args } => args.len() as u32 + 1,
                InstKind::MethodCall { args, .. } => args.len() as u32,

                InstKind::BuildArray { elements } => elements.len() as u32,
                // Object literals stage non-contiguous values into the call
                // area before BuildObjectWithShape.
                InstKind::BuildObject { pairs } => pairs.len() as u32,

                // Reserved unconditionally, including for calls that end up on
                // the windowless `IntrinsicDirect` form. The reservation must
                // be an UPPER bound on what emission uses: over-reserving
                // wastes a frame slot, under-reserving would hand the emitted
                // window registers that overlap live values.
                InstKind::IntrinsicCall { args, .. } => args.len() as u32 + 1,
                InstKind::CallNativeOp { args, .. } => args.len() as u32 + 1,

                InstKind::IterCall { .. } => 1,

                InstKind::SuperCall { args } => args.len() as u32 + 2,
                InstKind::SuperMethodCall { args, .. } => args.len() as u32 + 1,

                InstKind::ExtensionCall { args, .. } => args.len() as u32 + 2,
                InstKind::CallSpread { args, .. } => args.len() as u32 + 1,
                _ => 0,
            };
            max_call = max_call.max(t);
        }
    }

    let total = next + 2 + max_call;
    if total > 256 {
        return Err(OptError::Unsupported(
            "ssa-emit: register count exceeds 255",
        ));
    }
    let scratch = next as u8;
    let null_reg = (next + 1) as u8;
    let call_base = (next + 2) as u8;
    let register_count = total.max(1) as u16;
    Ok((reg, scratch, null_reg, call_base, register_count))
}

pub(super) fn var_reg(var: VarId, nparams: usize) -> u8 {
    match var {
        VarId::Param(i) => (1 + i) as u8,
        VarId::Local(id) => (1 + nparams + id.0 as usize) as u8,
    }
}
