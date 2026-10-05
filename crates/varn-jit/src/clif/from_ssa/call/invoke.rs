use cranelift_codegen::ir::{types, InstBuilder, Value};
use cranelift_frontend::FunctionBuilder;

use super::super::super::emit::call_helper;
use super::super::{heap, Ctx};
use super::direct::{entry_out_slot, run_entered_or};
use super::scratch::scratch_addr;

pub(crate) fn boxed_window(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    callee: Value,
    args: &[u32],
) -> Result<Value, String> {
    let addr = scratch_addr(b, ctx, args.len() + 1);
    b.ins().store(
        cranelift_codegen::ir::MemFlagsData::trusted(),
        callee,
        addr,
        0,
    );
    for (i, v) in args.iter().enumerate() {
        let boxed = heap::boxed_value(b, ctx, values, *v)?;
        b.ins().store(
            cranelift_codegen::ir::MemFlagsData::trusted(),
            boxed,
            addr,
            ((i + 1) * 16) as i32,
        );
    }
    Ok(addr)
}

pub(crate) fn emit_invoke(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    window: Value,
    callee: (Value, Value),
    argc: usize,
) -> Value {
    let frame = ctx.frame.as_ref().expect("a call has a frame");
    let (ctag, cpayload) = callee;
    let argc_v = b.ins().iconst(types::I64, argc as i64);
    let out = entry_out_slot(b);
    let entry = call_helper(
        b,
        ctx.cc,
        ctx.helpers.jit_invoke_window,
        &[frame.exec_ctx, ctag, cpayload, window, argc_v, out],
    );
    run_entered_or(b, ctx, entry, out)
}
