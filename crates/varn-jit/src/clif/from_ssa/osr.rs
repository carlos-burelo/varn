//! On-stack replacement entry: resuming a running interpreted frame at a loop
//! header, in compiled code.
//!
//! The interpreter reaches the header with its parameters and every value
//! live into it in their registers — the register allocator keeps a value's
//! register while it is live, and the compiler lists those values with the
//! header ([`varn_types::ssa::SsaLoopHeader`]). The entry reads them from
//! their homes into their native representation, and the header's
//! parameters become the jump's arguments. The body is the ordinary lowering
//! of the blocks reachable from the header.
//!
//! A value live into the header whose definition the resumed body executes
//! again — an outer loop's counter when the header is an inner loop's — has
//! two sources: the entry for the first iteration, the body afterwards. It is
//! a Cranelift variable, defined by both, so the header merges them.

use std::collections::HashMap;

use cranelift_codegen::ir::{Block, BlockArg, InstBuilder, Value};
use cranelift_frontend::{FunctionBuilder, Variable};
use varn_types::ssa::{SsaLoopHeader, SsaProto};

use super::store::{clif_ty, define, is_heap, load_home_value};
use super::Ctx;

/// The scalars live into `header` that the resumed body redefines, each with
/// its variable. `reached` is what the body compiles: the blocks reachable
/// from the header.
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

/// Read the header's live values and parameters from their homes and jump to
/// `header_blk`. The builder is in the function's entry block.
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
