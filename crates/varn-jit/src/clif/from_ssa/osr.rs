//! On-stack replacement entry: resuming a running interpreted frame at a loop
//! header, in compiled code.
//!
//! The interpreter reaches the header with its parameters and every value
//! live into it in their registers — the register allocator keeps a value's
//! register while it is live, and the compiler lists those values with the
//! header ([`varn_types::ssa::SsaLoopHeader`]). The entry reads them from
//! their homes: heap values need nothing (they live there anyway), scalars
//! are loaded into their native representation, and the header's
//! parameters become the jump's arguments. The body is the ordinary lowering
//! of the blocks reachable from the header.
//!
//! A scalar live into the header whose definition the resumed body executes
//! again — an outer loop's counter when the header is an inner loop's — has
//! two sources: the entry for the first iteration, the body afterwards. It is
//! a Cranelift variable, defined by both, so the header merges them.

use std::collections::HashMap;

use cranelift_codegen::ir::{Block, BlockArg, InstBuilder, Value};
use cranelift_frontend::{FunctionBuilder, Variable};
use varn_types::ssa::{SsaLoopHeader, SsaProto};

use super::store::{clif_ty, define_scalar, is_heap, load_home_value, use_heap};
use super::{heap, Ctx};

/// The block defining each value: a block parameter's block, or the block of
/// the instruction producing it.
fn def_blocks(ssa: &SsaProto) -> Vec<Option<usize>> {
    let mut def = vec![None; ssa.values.len()];
    for (b, blk) in ssa.blocks.iter().enumerate() {
        for &p in &blk.params {
            def[p as usize] = Some(b);
        }
        for d in blk.insts.iter().filter_map(|i| i.dest) {
            def[d as usize] = Some(b);
        }
    }
    def
}

/// The scalars live into `header` that the resumed body redefines, each with
/// its variable. `reached` is what the body compiles: the blocks reachable
/// from the header.
pub(super) fn carried(
    b: &mut FunctionBuilder,
    ssa: &SsaProto,
    header: &SsaLoopHeader,
    reached: &[bool],
) -> HashMap<u32, Variable> {
    let def = def_blocks(ssa);
    let mut carried = HashMap::new();
    for &v in &header.live {
        let kind = ssa.value_ty(v);
        let redefined = def[v as usize].is_some_and(|blk| reached[blk]);
        if redefined && !is_heap(kind) {
            if let Some(ty) = clif_ty(kind) {
                carried.insert(v, b.declare_var(ty));
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
        let kind = ssa.value_ty(v);
        if !is_heap(kind) {
            let x = load_home_value(b, ctx, ssa.reg(v), kind)?;
            define_scalar(b, ctx, values, v, x);
        }
    }
    let mut args = Vec::new();
    for &p in &ssa.blocks[header.block as usize].params {
        let kind = ssa.value_ty(p);
        let boxed = use_heap(b, ctx, ssa.reg(p))?;
        let arg = if is_heap(kind) {
            boxed
        } else {
            heap::unbox_dest(b, kind, boxed)?
        };
        args.push(BlockArg::from(arg));
    }
    b.ins().jump(header_blk, &args);
    Ok(())
}
