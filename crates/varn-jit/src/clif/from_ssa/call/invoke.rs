use cranelift_codegen::ir::{
    condcodes::IntCC, types, InstBuilder, MemFlagsData, StackSlotData, StackSlotKind, Value,
};
use cranelift_frontend::FunctionBuilder;
use varn_types::vm_value::KIND_NULL;

use super::super::super::emit::{call_helper, call_helper_void};
use super::super::{heap, load_value, Ctx, Out};
use super::direct::{entry_out_slot, run_entered_or};
use super::native_entry::{self, NativeCall};
use super::scratch::scratch_addr;

/// `new Class(...)`. When the class's constructor is compiled natively the
/// runtime only allocates the instance and the constructor is entered
/// directly with it as `this`; otherwise the whole construction runs in
/// `jit_new_window`. A constructor's `null` return means the instance.
pub(crate) fn emit_new(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    callee: u32,
    args: &[u32],
) -> Result<Out, String> {
    let frame = ctx.frame.as_ref().ok_or("from_ssa: new without a frame")?;
    let m = MemFlagsData::trusted();
    let ectx = frame.exec_ctx;
    let callee_v = load_value(b, ctx, values, callee)?;
    let (ct, cp) = b.ins().isplit(callee_v);
    let slot = b.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 32, 4));
    let out = b.ins().stack_addr(types::I64, slot, 0);
    let began = call_helper(b, ctx.cc, ctx.helpers.jit_new_begin, &[ectx, ct, cp, out]);
    let merge = b.create_block();
    b.append_block_param(merge, types::I128);
    let native = b.create_block();
    let generic = b.create_block();
    b.ins().brif(began, native, &[], generic, &[]);

    b.switch_to_block(native);
    let instance = b.ins().load(types::I128, m, out, 0);
    let ctor = b.ins().load(types::I128, m, out, 16);
    b.declare_value_needs_stack_map(instance);
    b.declare_value_needs_stack_map(ctor);
    let call = NativeCall {
        callee: ctor,
        receiver: instance,
        args,
        dest: None,
    };
    let returned = native_entry::emit(b, ctx, values, call, |b| {
        let window = boxed_window(b, ctx, values, instance, args)?;
        let argc = b.ins().iconst(types::I64, (args.len() + 1) as i64);
        call_helper_void(
            b,
            ctx.cc,
            ctx.helpers.jit_run_constructor,
            &[ectx, window, argc],
        );
        Ok(b.ins().load(
            types::I128,
            m,
            ectx,
            ctx.helpers.jit_native_result_offset as i32,
        ))
    })?;
    let returned = match returned {
        Out::Native(v) | Out::Boxed(v) => v,
    };
    let (rtag, rpayload) = b.ins().isplit(returned);
    let (itag, ipayload) = b.ins().isplit(instance);
    let is_null = b.ins().icmp_imm_u(IntCC::Equal, rtag, KIND_NULL as i64);
    let tag = b.ins().select(is_null, itag, rtag);
    let payload = b.ins().select(is_null, ipayload, rpayload);
    let built = b.ins().iconcat(tag, payload);
    b.ins().jump(merge, &[built.into()]);

    b.switch_to_block(generic);
    let mut vals = Vec::with_capacity(args.len());
    for a in args {
        vals.push(heap::boxed_value(b, ctx, values, *a)?);
    }
    let addr = scratch_addr(b, ctx, vals.len().max(1));
    for (i, v) in vals.iter().enumerate() {
        b.ins().store(m, *v, addr, (i * 16) as i32);
    }
    let argc = b.ins().iconst(types::I64, vals.len() as i64);
    call_helper_void(
        b,
        ctx.cc,
        ctx.helpers.jit_new_window,
        &[ectx, ct, cp, addr, argc],
    );
    let built = b.ins().load(
        types::I128,
        m,
        ectx,
        ctx.helpers.jit_native_result_offset as i32,
    );
    super::super::store::drop_home_addrs(ctx);
    b.ins().jump(merge, &[built.into()]);

    b.switch_to_block(merge);
    Ok(Out::Boxed(b.block_params(merge)[0]))
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
