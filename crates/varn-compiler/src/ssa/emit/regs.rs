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

    // Kind-aware linear scan: each `SlotKind` draws from its own free-register
    // pool, so a physical register is not shared between values of different
    // kinds while there is room to avoid it.
    //
    // The reason is `derive_register_meta`, which meets the kinds of every SSA
    // value assigned to a register and yields `Dynamic` the moment two differ.
    // Live ranges being disjoint, a kind-agnostic allocator packs them freely --
    // and one such packing is enough to erase the kind for the whole function.
    // It cost real time: a matmul's `b[k * n + col]` landed the loop's `Bool`
    // comparison and the load's `Int` destination in one register, the meet
    // returned `Dynamic`, and the JIT lost the repr specialisation that lets a
    // loop body skip the per-access representation branch. 15 ms against 3 ms
    // for the identical loop whose registers happened not to collide.
    //
    // This generalises a split that was already here for `Float` alone, for
    // exactly the same reason.
    //
    // Where it differs from that one: the split is a preference, not a rule.
    // Separate pools mean a value that finds its own pool empty takes a fresh
    // register, so total pressure grows -- and a function over 255 registers is
    // rejected outright by the caller, which is a far worse outcome than a lost
    // kind. So a fresh register is only taken while one fits under the ceiling
    // the call window leaves; past that, the scan falls back to reusing another
    // kind's register and accepts the `Dynamic` meet, exactly as before.
    let kind_of_v = |v: usize| slot_kind_of(ssa.values[v].ty);
    // Highest register the frame can hand out: `total` below is
    // `next + 2 + max_call`, and the caller rejects the function past 256.
    let ceiling = 256u32.saturating_sub(2 + max_call);
    let mut next: u32 = base;
    // One free list per `SlotKind`, indexed by `pool_index`.
    let mut free: [Vec<u32>; POOLS] = Default::default();
    // (interval end, register, kind index)
    let mut active: Vec<(u32, u32, usize)> = Vec::new();
    for &v in &order {
        let d = def[v];
        let vk = pool_index(kind_of_v(v));
        let mut i = 0;
        while i < active.len() {
            if active[i].0 < d {
                let (_, r, rk) = active[i];
                free[rk].push(r);
                active.swap_remove(i);
            } else {
                i += 1;
            }
        }
        let r = if let Some(m) = free[vk].iter().copied().min() {
            free[vk].retain(|&x| x != m);
            m
        } else if next < ceiling {
            let r = next;
            next += 1;
            r
        } else {
            // Out of room: reuse the lowest register any other kind has freed,
            // losing that register's kind rather than the whole function.
            let borrowed = free
                .iter()
                .enumerate()
                .filter_map(|(k, p)| p.iter().copied().min().map(|m| (m, k)))
                .min();
            match borrowed {
                Some((m, k)) => {
                    free[k].retain(|&x| x != m);
                    m
                }
                None => {
                    let r = next;
                    next += 1;
                    r
                }
            }
        };
        if r > 255 {
            return Err(OptError::Unsupported(
                "ssa-emit: register count exceeds 255",
            ));
        }
        reg[v] = r as u8;
        assigned[v] = true;
        active.push((end[v], r, vk));
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

/// Number of `SlotKind` variants, and so of free-register pools.
const POOLS: usize = 6;

/// Dense index of a `SlotKind`, for the per-kind free lists.
fn pool_index(k: varn_types::register_meta::SlotKind) -> usize {
    use varn_types::register_meta::SlotKind;
    match k {
        SlotKind::Int => 0,
        SlotKind::Float => 1,
        SlotKind::Bool => 2,
        SlotKind::Str => 3,
        SlotKind::Ref | SlotKind::Class(_) | SlotKind::Array(_) => 4,
        SlotKind::Nullable(_) | SlotKind::Dynamic => 5,
    }
}
