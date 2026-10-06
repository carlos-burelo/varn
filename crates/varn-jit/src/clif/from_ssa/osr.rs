















use std::collections::HashMap;

use cranelift_codegen::ir::{Block, BlockArg, InstBuilder, Value};
use cranelift_frontend::{FunctionBuilder, Variable};
use varn_types::ssa::{SsaLoopHeader, SsaProto};

use super::store::{clif_ty, define, is_heap, load_home_value};
use super::Ctx;




pub(super) fn carried(
    b: &mut FunctionBuilder,
    ssa: &SsaProto,
    header: &SsaLoopHeader,
    reached: &[bool],
) -> HashMap<u32, Variable> {
    let def = super::cfg::def_blocks(ssa);
    let mut carried = HashMap::new();
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



pub(super) fn emit_entry(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &mut [Option<Value>],
    header: &SsaLoopHeader,
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
