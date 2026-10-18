use std::collections::BTreeMap;

use cranelift_codegen::ir::{types, Block, BlockArg, InstBuilder, Value};
use cranelift_frontend::FunctionBuilder;
use rustc_hash::{FxHashMap, FxHashSet};
use varn_types::ssa::{SsaProto, SsaTerm};

use super::super::native_abi::NativeShape;
use super::block::lower_block;
use super::range_versioning::VersionPlan;
use super::{arrays, cfg, setup, store, Ctx};

pub(super) struct VersionClones {
    fast: BTreeMap<usize, Block>,
    slow: BTreeMap<usize, Block>,
    fast_view: Vec<Option<Block>>,
    slow_view: Vec<Option<Block>>,
}

pub(super) fn build_clones(
    b: &mut FunctionBuilder,
    ssa: &SsaProto,
    plans: &[VersionPlan],
    preds: &[Vec<usize>],
    rpo_pos: &[usize],
    reached: &[bool],
    blocks: &[Option<Block>],
) -> Result<Vec<VersionClones>, String> {
    let mut vmaps: Vec<VersionClones> = Vec::new();
    for pl in plans {
        let body = cfg::loop_body(preds, rpo_pos, reached, pl.header);
        let mut fast = BTreeMap::new();
        let mut slow = BTreeMap::new();
        for s in &body {
            for (map, _) in [(&mut fast, true), (&mut slow, false)] {
                let cb = b.create_block();
                for &p in &ssa.blocks[*s].params {
                    let ty = store::clif_ty(ssa.value_ty(p))
                        .ok_or("from_ssa: unsupported block param")?;
                    b.append_block_param(cb, ty);
                }
                map.insert(*s, cb);
            }
        }
        let mut fast_view = blocks.to_vec();
        let mut slow_view = blocks.to_vec();
        for (s, c) in &fast {
            fast_view[*s] = Some(*c);
        }
        for (s, c) in &slow {
            slow_view[*s] = Some(*c);
        }
        vmaps.push(VersionClones {
            fast,
            slow,
            fast_view,
            slow_view,
        });
    }
    Ok(vmaps)
}

pub(super) fn emit_version_guard(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    ssa: &SsaProto,
    plan: &VersionPlan,
    vm: &VersionClones,
) -> Result<(), String> {
    use cranelift_codegen::ir::condcodes::IntCC;
    let mut ok = b.ins().iconst(types::I64, 1);
    for (object, need) in &plan.guards {
        let view = ctx
            .views
            .of(*object)
            .ok_or("from_ssa: version guard without array view")?;
        let len_v = b.use_var(view.len);
        let need_v = b.ins().iconst(types::I64, *need as i64);
        let c = b
            .ins()
            .icmp(IntCC::UnsignedGreaterThanOrEqual, len_v, need_v);
        let c = b.ins().uextend(types::I64, c);
        ok = b.ins().band(ok, c);
    }
    let args: Vec<BlockArg> = match &ssa.blocks[plan.preheader].term {
        SsaTerm::Jump { args, .. } => args
            .iter()
            .map(|v| store::load_value(b, ctx, values, *v).map(BlockArg::from))
            .collect::<Result<_, _>>()?,
        SsaTerm::Return(_) | SsaTerm::Throw(_) | SsaTerm::Branch { .. } | SsaTerm::Unreachable => {
            return Err("from_ssa: version preheader without jump".into())
        }
    };
    let fh = vm.fast.get(&plan.header).copied().expect("fast header");
    let sh = vm.slow.get(&plan.header).copied().expect("slow header");
    b.ins().brif(ok, fh, &args, sh, &args);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn lower_versioned(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &mut [Option<Value>],
    blocks: &[Option<Block>],
    emitted: &mut [bool],
    ssa: &SsaProto,
    preds: &[Vec<usize>],
    rpo: &[usize],
    rpo_pos: &[usize],
    shape: &NativeShape,
    global_src: &BTreeMap<u32, u32>,
    pins: &BTreeMap<usize, Vec<u32>>,
    hoisted: &mut BTreeMap<(usize, u32), Value>,
    hoist_at: &BTreeMap<usize, Vec<(usize, u32)>>,
    loop_bodies: &FxHashMap<usize, FxHashSet<usize>>,
    landing_live: &setup::LandingLive,
    plans: &[VersionPlan],
    vmaps: &[VersionClones],
    pi: usize,
    i: usize,
    n: usize,
    entry: usize,
) -> Result<(), String> {
    emitted[i] = true;
    let gobjs: Vec<u32> = plans[pi].guards.iter().map(|(o, _)| *o).collect();
    if !gobjs.iter().all(|o| values[*o as usize].is_some()) {
        let cb = blocks[i].expect("block created");
        return lower_block(
            b,
            ctx,
            values,
            blocks,
            ssa,
            preds,
            rpo_pos,
            shape,
            global_src,
            pins,
            hoisted,
            hoist_at,
            loop_bodies,
            landing_live,
            i,
            cb,
            i == entry,
            true,
            false,
            false,
            false,
            n,
            false,
        )
        .map_err(|e| format!("{e} [bloque {i}]"));
    }
    let cb = blocks[i].expect("block created");
    lower_block(
        b,
        ctx,
        values,
        blocks,
        ssa,
        preds,
        rpo_pos,
        shape,
        global_src,
        pins,
        hoisted,
        hoist_at,
        loop_bodies,
        landing_live,
        i,
        cb,
        i == entry,
        true,
        false,
        false,
        true,
        n,
        true,
    )
    .map_err(|e| format!("{e} [preheader {i} del plan {pi}]"))?;
    arrays::prefill(b, ctx, values, &gobjs).map_err(|e| format!("{e} [prefill del plan {pi}]"))?;
    emit_version_guard(b, ctx, values, ssa, &plans[pi], &vmaps[pi])
        .map_err(|e| format!("{e} [guard del plan {pi}]"))?;
    let mut fast_order: Vec<(usize, Block)> = vmaps[pi]
        .fast
        .iter()
        .filter(|(s, _)| rpo.contains(s))
        .map(|(s, c)| (*s, *c))
        .collect();
    fast_order.sort_by_key(|(s, _)| rpo_pos[*s]);
    let mut fast_values = values.to_vec();
    for (ssa_b, fast_blk) in fast_order {
        lower_block(
            b,
            ctx,
            &mut fast_values,
            &vmaps[pi].fast_view,
            ssa,
            preds,
            rpo_pos,
            shape,
            global_src,
            pins,
            hoisted,
            hoist_at,
            loop_bodies,
            landing_live,
            ssa_b,
            fast_blk,
            false,
            false,
            true,
            true,
            false,
            n,
            false,
        )
        .map_err(|e| format!("{e} [clon fast {ssa_b} del plan {pi}]"))?;
        emitted[ssa_b] = true;
    }
    let mut slow_order: Vec<(usize, Block)> = vmaps[pi]
        .slow
        .iter()
        .filter(|(s, _)| rpo.contains(s))
        .map(|(s, c)| (*s, *c))
        .collect();
    slow_order.sort_by_key(|(s, _)| rpo_pos[*s]);
    let mut slow_values = values.to_vec();
    for (ssa_b, slow_blk) in slow_order {
        lower_block(
            b,
            ctx,
            &mut slow_values,
            &vmaps[pi].slow_view,
            ssa,
            preds,
            rpo_pos,
            shape,
            global_src,
            pins,
            hoisted,
            hoist_at,
            loop_bodies,
            landing_live,
            ssa_b,
            slow_blk,
            false,
            false,
            false,
            true,
            false,
            n,
            false,
        )
        .map_err(|e| format!("{e} [clon slow {ssa_b} del plan {pi}]"))?;
        emitted[ssa_b] = true;
    }
    Ok(())
}
