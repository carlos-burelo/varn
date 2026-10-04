use cranelift_codegen::ir::{types, InstBuilder, Value};
use cranelift_frontend::FunctionBuilder;

use super::super::super::emit::call_helper;
use super::super::{load_value, Ctx, Out};
use super::direct::{entry_out_slot, run_entered_or};
use super::invoke::{boxed_window, emit_invoke};
use super::native_entry::{self, NativeCall};

pub(crate) fn emit_call(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    callee: u32,
    args: &[u32],
    dest: Option<u32>,
) -> Result<Out, String> {
    if !super::super::store::is_heap(ctx.ssa.value_ty(callee)) {
        return Err("from_ssa: call through a scalar callee".into());
    }
    let callee_v = load_value(b, ctx, values, callee)?;
    let call = NativeCall {
        callee: callee_v,
        receiver: callee_v,
        args,
        dest,
    };
    native_entry::emit(b, ctx, values, call, |b| {
        let window = boxed_window(b, ctx, values, callee_v, args)?;
        let (ctag, cpayload) = b.ins().isplit(callee_v);
        Ok(emit_invoke(
            b,
            ctx,
            window,
            (ctag, cpayload),
            args.len() + 1,
        ))
    })
}

pub(crate) fn emit_self_call_framed(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    args: &[u32],
) -> Result<Value, String> {
    let frame = ctx
        .frame
        .as_ref()
        .ok_or("from_ssa: framed self-call without a frame")?;
    if args.len() + 1 != ctx.proto.arity {
        return Err("from_ssa: self-call arity mismatch".into());
    }
    let receiver = super::super::props::emit_this(ctx)?;
    let window = boxed_window(b, ctx, values, receiver, args)?;
    let argc = b.ins().iconst(types::I64, (args.len() + 1) as i64);
    let out = entry_out_slot(b);
    let entry = call_helper(
        b,
        ctx.cc,
        ctx.helpers.jit_call_self_window,
        &[frame.exec_ctx, window, argc, out],
    );
    Ok(run_entered_or(b, ctx, entry, out))
}
