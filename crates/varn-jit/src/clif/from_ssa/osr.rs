use rustc_hash::FxHashMap;

use cranelift_codegen::ir::{types, Block, BlockArg, InstBuilder, MemFlagsData, Value};
use cranelift_frontend::{FunctionBuilder, Variable};
use varn_types::ssa::{SsaOp, SsaProto, SsaSecondaryEntry, SsaTerm};

use super::store::{clif_ty, define, is_heap, land, load_home_value};
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
        if redefined && let Some(ty) = clif_ty(kind) {
            let var = b.declare_var(ty);
            if is_heap(kind) {
                b.declare_var_needs_stack_map(var);
            }
            carried.insert(v, var);
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
    let consts: FxHashMap<u32, &SsaOp> = ssa
        .blocks
        .iter()
        .flat_map(|blk| blk.insts.iter())
        .filter(|inst| {
            matches!(
                inst.op,
                SsaOp::ConstInt(_) | SsaOp::ConstFloat(_) | SsaOp::ConstBool(_)
            )
        })
        .filter_map(|inst| Some((inst.dest?, &inst.op)))
        .collect();
    for &v in &header.live {
        if let Some(op) = consts.get(&v) {
            if let Some(out) = super::scalar::emit_inst(b, ctx, values, op, Some(v))? {
                land(b, ctx, values, Some(v), out)?;
            }
            continue;
        }
        let x = load_home_value(b, ctx, ssa.reg(v), ssa.value_ty(v))?;
        define(b, ctx, values, v, x);
    }
    {
        let flag = b.ins().load(
            types::I64,
            MemFlagsData::trusted(),
            ctx.exec_ctx,
            ctx.helpers.jit_error_flag_offset as i32,
        );
        let (landing, err) = {
            let st = ctx.pure_try.borrow();
            let stack = st
                .entry_stacks
                .get(header.block as usize)
                .cloned()
                .unwrap_or_default();
            let landing = stack.last().and_then(|h| st.clif_map.get(h).copied());
            (landing, st.err_ret)
        };
        let resume = b.create_block();
        match (landing, err) {
            (Some(t), _) => {
                let rej = b.create_block();
                b.ins().brif(flag, rej, &[], resume, &[]);
                b.switch_to_block(rej);
                let boxed = b.ins().load(
                    types::I128,
                    MemFlagsData::trusted(),
                    ctx.exec_ctx,
                    ctx.helpers.jit_native_result_offset as i32,
                );
                b.declare_value_needs_stack_map(boxed);
                let zero = b.ins().iconst(types::I64, 0);
                b.ins().store(
                    MemFlagsData::trusted(),
                    zero,
                    ctx.exec_ctx,
                    ctx.helpers.jit_error_flag_offset as i32,
                );
                b.ins().jump(t, &[boxed.into()]);
                b.switch_to_block(resume);
            }
            (None, Some(e)) => {
                b.ins().brif(flag, e, &[], resume, &[]);
                b.switch_to_block(resume);
            }
            (None, None) => {}
        }
    }
    let mut args = Vec::new();
    for &p in &ssa.blocks[header.block as usize].params {
        let arg = load_home_value(b, ctx, ssa.reg(p), ssa.value_ty(p))?;
        args.push(BlockArg::from(arg));
    }
    b.ins().jump(header_blk, &args);
    Ok(())
}

pub(super) fn expand_resume(
    ssa: &SsaProto,
    entry_stacks: &[Vec<usize>],
    resume_blk: usize,
    rpo: &mut Vec<usize>,
    rpo_pos: &mut [usize],
    reached: &mut [bool],
) {
    let enclosing = entry_stacks.get(resume_blk).cloned().unwrap_or_default();
    let mut stack: Vec<usize> = enclosing;
    while let Some(b) = stack.pop() {
        if b >= reached.len() || reached[b] {
            continue;
        }
        reached[b] = true;
        rpo_pos[b] = rpo.len();
        rpo.push(b);
        match &ssa.blocks[b].term {
            SsaTerm::Jump { target, .. } => {
                let s = *target as usize;
                if s < reached.len() && !reached[s] {
                    stack.push(s);
                }
            }
            SsaTerm::Branch {
                then_blk, else_blk, ..
            } => {
                for s in [*then_blk as usize, *else_blk as usize] {
                    if s < reached.len() && !reached[s] {
                        stack.push(s);
                    }
                }
            }
            SsaTerm::Return(_) | SsaTerm::Throw(_) | SsaTerm::Unreachable => {}
        }
        for inst in &ssa.blocks[b].insts {
            if let varn_types::ssa::SsaOp::Try { catch_ip, .. } = &inst.op
                && let Some(e) = ssa.catch_at(*catch_ip as usize)
            {
                let h = e.block as usize;
                if h < reached.len() && !reached[h] {
                    stack.push(h);
                }
            }
        }
    }
}
