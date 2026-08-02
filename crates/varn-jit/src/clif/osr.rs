//! On-stack replacement entry: resuming a RUNNING interpreter frame in
//! compiled code.
//!
//! The normal entry is a function call — arguments arrive in registers and
//! execution starts at bytecode ip 0. An OSR entry is not a call: the frame
//! already exists, its registers already hold values, and execution must
//! resume at the loop header that proved hot. So the lowering emits the same
//! body with a different prologue — no parameters, every register
//! materialized from its `ctx.stack` home slot, and a jump straight to the
//! CLIF block for `osr_ip` instead of block 0.
//!
//! That is the whole difference. Everything downstream (the block walk, the
//! kind dataflow, the safepoints, the return path) is byte-for-byte the
//! normal lowering, which is what keeps the two tiers in agreement.

use cranelift_codegen::ir::{Block, InstBuilder};
use cranelift_frontend::FunctionBuilder;

use super::alloc::{self, AllocCtx};
use super::kinds::K;

/// Materialize every register from its `ctx.stack` home slot into the
/// representation the OSR target block expects, then jump there.
///
/// `target_state` MUST be `kind_flow`'s entry state for `osr_ip` — never
/// `register_meta`. The interpreter's frame slots always hold boxed
/// `VmValue`; the block at `osr_ip` expects whatever the dataflow merged
/// there, which for a loop header routinely disagrees with the meta (the meta
/// types a slot `Int` while the flow has merged it to a boxed kind, and vice
/// versa). Converting by the meta leaves a raw integer in a register the
/// lowering goes on to read as boxed `VmValue` bits — a small int
/// reinterpreted as a denormal float, which is silently wrong comparisons and
/// an INFINITE LOOP when the register is the loop counter. See the same
/// warning on [`alloc::reload_boxed`], which this reuses precisely so there is
/// only one implementation of the conversion.
///
/// Every register is reloaded, not just the live ones: liveness is indexed by
/// bytecode ip and describes what is readable *after* an instruction, whereas
/// here nothing has executed yet. A register the target block never reads is
/// dead code Cranelift removes.
pub(super) fn emit_osr_entry(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    target_state: &[K],
    target: Block,
) {
    let all: Vec<usize> = (0..actx.nregs).collect();
    alloc::reload_boxed(b, actx, target_state, &all);
    b.ins().jump(target, &[]);
}
