use rustc_hash::FxHashMap;

use cranelift_codegen::ir::{Block, BlockArg, InstBuilder, Value};
use cranelift_frontend::{FunctionBuilder, Variable};
use varn_types::ssa::{SsaProto, SsaSecondaryEntry};

use super::store::{clif_ty, define, is_heap, load_home_value};
use super::Ctx;

pub(super) fn carried(
    b: &mut FunctionBuilder,
    ssa: &SsaProto,
    header: &SsaSecondaryEntry,
    reached: &[bool],
) -> FxHashMap<u32, Variable> {
    let def = super::cfg::def_blocks(ssa);
    let mut carried = FxHashMap::default();
    for &v in &header.live {
        let kind = ssa.value_ty(v);
        let redefined = def[v as usize].is_some_and(|blk| reached[blk]);
        if redefined {
            if let Some(ty) = clif_ty(kind) {
                let var = b.declare_var(ty);
                if is_heap(kind) {
                    b.declare_var_needs_stack_map(var);
                }
                carried.insert(v, var);
            }
        }
    }
    carried
}

pub(super) fn versioned(
    b: &mut FunctionBuilder,
    ssa: &SsaProto,
    plans: &[super::ranges::VersionPlan],
    preds: &[Vec<usize>],
    rpo_pos: &[usize],
    reached: &[bool],
) -> FxHashMap<u32, Variable> {
    let mut carried = FxHashMap::default();
    for plan in plans {
        for blk in super::cfg::loop_body(preds, rpo_pos, reached, plan.header) {
            let blk = &ssa.blocks[blk];
            let defined = blk
                .params
                .iter()
                .copied()
                .chain(blk.insts.iter().filter_map(|inst| inst.dest));
            for v in defined {
                let kind = ssa.value_ty(v);
                if let Some(ty) = clif_ty(kind) {
                    let var = b.declare_var(ty);
                    if is_heap(kind) {
                        b.declare_var_needs_stack_map(var);
                    }
                    carried.insert(v, var);
                }
            }
        }
    }
    carried
}

pub(super) fn emit_entry(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &mut [Option<Value>],
    header: &SsaSecondaryEntry,
    header_blk: Block,
) -> Result<(), String> {
    let ssa = ctx.ssa;
    for &v in &header.live {
        let x = load_home_value(b, ctx, ssa.reg(v), ssa.value_ty(v))?;
        define(b, ctx, values, v, x);
    }
    let mut args = Vec::new();
    for &p in &ssa.blocks[header.block as usize].params {
        let arg = load_home_value(b, ctx, ssa.reg(p), ssa.value_ty(p))?;
        args.push(BlockArg::from(arg));
    }
    b.ins().jump(header_blk, &args);
    Ok(())
}
