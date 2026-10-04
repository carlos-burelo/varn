use cranelift_codegen::ir::{types, InstBuilder, Value};
use cranelift_frontend::FunctionBuilder;

use super::super::super::emit::{call_helper, call_helper_void};
use super::super::{heap, load_value, Ctx, Out};
use super::direct::{entry_out_slot, run_entered_or};
use super::scratch::scratch_addr;

pub(crate) fn emit_new(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    callee: u32,
    args: &[u32],
) -> Result<Out, String> {
    let frame = ctx.frame.as_ref().ok_or("from_ssa: new without a frame")?;
    let callee_v = load_value(b, ctx, values, callee)?;
    let (ct, cp) = b.ins().isplit(callee_v);
    let mut vals = Vec::with_capacity(args.len());
    for a in args {
        vals.push(heap::boxed_value(b, ctx, values, *a)?);
    }
    let addr = scratch_addr(b, ctx, vals.len().max(1));
    for (i, v) in vals.iter().enumerate() {
        b.ins().store(
            cranelift_codegen::ir::MemFlagsData::trusted(),
            *v,
            addr,
            (i * 16) as i32,
        );
    }
    let argc = b.ins().iconst(types::I64, vals.len() as i64);
    call_helper_void(
        b,
        ctx.cc,
        ctx.helpers.jit_new_window,
        &[frame.exec_ctx, ct, cp, addr, argc],
    );
    Ok(Out::Boxed(b.ins().load(
        types::I128,
        cranelift_codegen::ir::MemFlagsData::trusted(),
        frame.exec_ctx,
        ctx.helpers.jit_native_result_offset as i32,
    )))
}

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
