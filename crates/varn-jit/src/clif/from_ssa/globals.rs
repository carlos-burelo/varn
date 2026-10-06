












use cranelift_codegen::ir::{types, InstBuilder, Value};
use cranelift_frontend::FunctionBuilder;

use super::Ctx;


#[derive(Clone, Copy)]
pub(super) enum Region {
    
    Module,
    
    Native,
}


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
    let rcbox = b.ins().load(
        types::I64,
        cranelift_codegen::ir::MemFlagsData::trusted(),
        frame.exec_ctx,
        helpers.globals_offset as i32,
    );
    let gbase = b.ins().load(
        types::I64,
        cranelift_codegen::ir::MemFlagsData::trusted(),
        rcbox,
        helpers.globals_store_offset as i32,
    );
    let idx = b.ins().iconst(types::I64, i64::from(slot));
    let eff = match region {
        Region::Module => {
            let mb = b.ins().load(
                types::I32,
                cranelift_codegen::ir::MemFlagsData::trusted(),
                frame.closure,
                helpers.closure_module_base_offset as i32,
            );
            let mb = b.ins().uextend(types::I64, mb);
            b.ins().iadd(mb, idx)
        }
        Region::Native => idx,
    };
    let scaled = b.ins().imul_imm_u(eff, 16);
    Ok(b.ins().iadd(gbase, scaled))
}


pub(super) fn emit_load(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    slot: u32,
    region: Region,
) -> Result<Value, String> {
    let addr = slot_addr(b, ctx, slot, region)?;
    Ok(b.ins().load(
        types::I128,
        cranelift_codegen::ir::MemFlagsData::trusted(),
        addr,
        0,
    ))
}


pub(super) fn emit_store(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    slot: u32,
    value: Value,
) -> Result<(), String> {
    let addr = slot_addr(b, ctx, slot, Region::Module)?;
    b.ins().store(
        cranelift_codegen::ir::MemFlagsData::trusted(),
        value,
        addr,
        0,
    );
    Ok(())
}
