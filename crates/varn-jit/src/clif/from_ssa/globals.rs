//! Global reads and writes for the SSA lowering.
//!
//! Globals live in `ExecCtx.globals` (a `GlobalStore`); its `values` Vec data
//! pointer sits at `globals_offset`. A module global's slot is RELATIVE to
//! the running closure's module region (`closure.module_base`, at
//! `closure_module_base_offset`); a native (prelude) global's index is
//! absolute. Both offsets come from the one probed `JitHelpers` table, so
//! this lowering addresses exactly what the interpreter does. Globals are GC
//! roots: a write is a plain store, no barrier.

use cranelift_codegen::ir::{types, InstBuilder, MemFlags, Value};
use cranelift_frontend::FunctionBuilder;

use super::Ctx;

/// Which region of the global store a slot indexes.
#[derive(Clone, Copy)]
pub(super) enum Region {
    /// The running closure's module: `module_base + slot`.
    Module,
    /// The prelude / host globals: `slot` itself.
    Native,
}

/// Machine address of global `slot` of `region`.
fn slot_addr(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    slot: u32,
    region: Region,
) -> Result<Value, String> {
    let frame = ctx
        .frame
        .as_ref()
        .ok_or("from_ssa: global access in a frame-less body")?;
    let helpers = ctx.helpers;
    let gbase = b.ins().load(
        types::I64,
        MemFlags::trusted(),
        frame.exec_ctx,
        helpers.globals_offset as i32,
    );
    let idx = b.ins().iconst(types::I64, i64::from(slot));
    let eff = match region {
        Region::Module => {
            let mb = b.ins().load(
                types::I32,
                MemFlags::trusted(),
                frame.closure,
                helpers.closure_module_base_offset as i32,
            );
            let mb = b.ins().uextend(types::I64, mb);
            b.ins().iadd(mb, idx)
        }
        Region::Native => idx,
    };
    let scaled = b.ins().imul_imm(eff, 16);
    Ok(b.ins().iadd(gbase, scaled))
}

/// Global `slot` of `region`, boxed.
pub(super) fn emit_load(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    slot: u32,
    region: Region,
) -> Result<Value, String> {
    let addr = slot_addr(b, ctx, slot, region)?;
    Ok(b.ins().load(types::I128, MemFlags::trusted(), addr, 0))
}

/// Write boxed `value` to module global `slot`.
pub(super) fn emit_store(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    slot: u32,
    value: Value,
) -> Result<(), String> {
    let addr = slot_addr(b, ctx, slot, Region::Module)?;
    b.ins().store(MemFlags::trusted(), value, addr, 0);
    Ok(())
}
