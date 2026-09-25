//! Module-relative global reads for the SSA lowering.
//!
//! Globals live in `ExecCtx.globals` (a `GlobalStore`); its `values` Vec data
//! pointer sits at `globals_offset` and the slot a `LoadGlobalIdx` names is
//! RELATIVE to the running closure's module region
//! (`closure.module_base`, at `closure_module_base_offset`). Both offsets come
//! from the one probed `JitHelpers` table, so this lowering addresses exactly
//! what the interpreter and the bytecode lowering do.

use cranelift_codegen::ir::{types, InstBuilder, MemFlags, Value};
use cranelift_frontend::FunctionBuilder;

use super::Ctx;

pub(super) fn emit_load(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    slot: u32,
) -> Result<Value, String> {
    let frame = ctx
        .frame
        .as_ref()
        .ok_or("from_ssa: global load in a frame-less body")?;
    let helpers = ctx.helpers;

    let gbase = b.ins().load(
        types::I64,
        MemFlags::trusted(),
        frame.exec_ctx,
        helpers.globals_offset as i32,
    );
    let mb = b.ins().load(
        types::I32,
        MemFlags::trusted(),
        frame.closure,
        helpers.closure_module_base_offset as i32,
    );
    let mb = b.ins().uextend(types::I64, mb);
    let idx = b.ins().iconst(types::I64, slot as i64);
    let eff = b.ins().iadd(mb, idx);
    let scaled = b.ins().imul_imm(eff, 16);
    let addr = b.ins().iadd(gbase, scaled);
    Ok(b.ins().load(types::I128, MemFlags::trusted(), addr, 0))
}
