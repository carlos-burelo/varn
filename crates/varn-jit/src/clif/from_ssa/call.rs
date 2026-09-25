//! Calls for the SSA lowering: direct self-recursion and cross-proto calls.
//!
//! `SelfCall` is handled in [`super::scalar`] (a leaf body's direct hardware
//! call). `Call` needs the frame (`closure` to read the callee global,
//! `exec_ctx` for the canonical path). Every call can take the canonical
//! `ExecCtx::invoke` through `jit_invoke_window`, whatever the callee (closure,
//! class, native) and whatever its arguments: the boxed window is copied into
//! the VM's staging area — a GC root — before anything can allocate. When the
//! linker resolves the callee to a compiled leaf with this exact scalar
//! signature, a guarded direct call runs first.

use cranelift_codegen::ir::{
    condcodes::IntCC, types, AbiParam, InstBuilder, MemFlags, Signature, StackSlotData,
    StackSlotKind, Value,
};
use cranelift_frontend::FunctionBuilder;
use varn_types::register_meta::SlotKind;
use varn_types::vm_value::KIND_HEAP;

use super::{heap, load_value, Ctx, Out};

use super::super::emit::{box_int, call_helper, call_helper_void};

fn is_scalar(k: SlotKind) -> bool {
    matches!(k, SlotKind::Int | SlotKind::Float | SlotKind::Bool)
}

pub(super) fn emit_call(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    callee: u32,
    callee_global: Option<u32>,
    args: &[u32],
    dest: Option<u32>,
) -> Result<Out, String> {
    let frame = ctx
        .frame
        .as_ref()
        .ok_or("from_ssa: call in a frame-less body")?;

    let callee_v = load_value(b, ctx, values, callee)?;
    let arg_kinds: Vec<SlotKind> = args.iter().map(|v| ctx.ssa.value_ty(*v)).collect();

    // The direct path: a published leaf with exactly these scalar argument
    // classes and a scalar return landing in a scalar destination.
    let dest_ty = dest.map(|d| ctx.ssa.value_ty(d));
    let direct = callee_global
        .and_then(|slot| frame.linker.static_target(slot as usize))
        .filter(|t| {
            dest_ty.is_some_and(is_scalar)
                && is_scalar(t.return_kind)
                && t.param_kinds == arg_kinds
                && arg_kinds.iter().all(|k| is_scalar(*k))
        });
    let Some(target) = direct else {
        let window = boxed_window(b, ctx, values, callee_v, args)?;
        return Ok(Out::Boxed(emit_invoke(b, ctx, window, args.len() + 1)));
    };
    let dest_ty = dest_ty.expect("filtered to a scalar destination");
    let merge_ty = match dest_ty {
        SlotKind::Float => types::F64,
        _ => types::I64,
    };

    let (ctag, cpayload) = b.ins().isplit(callee_v);
    let expected_tag = b.ins().iconst(types::I64, KIND_HEAP as i64);
    let expected_payload = b.ins().iconst(types::I64, target.expected_bits as i64);
    let same_tag = b.ins().icmp(IntCC::Equal, ctag, expected_tag);
    let same_payload = b.ins().icmp(IntCC::Equal, cpayload, expected_payload);
    let slot_addr = b.ins().iconst(types::I64, target.raw_slot as i64);
    let raw = b.ins().load(types::I64, MemFlags::trusted(), slot_addr, 0);
    let published = b.ins().icmp_imm(IntCC::NotEqual, raw, 0);
    let both = b.ins().band(same_tag, same_payload);
    let take_direct = b.ins().band(both, published);

    let fast = b.create_block();
    let slow = b.create_block();
    let merge = b.create_block();
    b.append_block_param(merge, merge_ty);
    b.ins().brif(take_direct, fast, &[], slow, &[]);

    b.switch_to_block(fast);
    let arg_values: Vec<Value> = args
        .iter()
        .map(|v| load_value(b, ctx, values, *v))
        .collect::<Result<_, _>>()?;
    let direct = {
        let mut sig = Signature::new(ctx.cc);
        for k in &target.param_kinds {
            sig.params.push(AbiParam::new(match k {
                SlotKind::Float => types::F64,
                _ => types::I64,
            }));
        }
        sig.returns.push(AbiParam::new(match target.return_kind {
            SlotKind::Float => types::F64,
            _ => types::I64,
        }));
        let sig_ref = b.import_signature(sig);
        let call = b.ins().call_indirect(sig_ref, raw, &arg_values);
        b.inst_results(call)[0]
    };
    b.ins().jump(merge, &[direct.into()]);

    b.switch_to_block(slow);
    let window = boxed_window(b, ctx, values, callee_v, args)?;
    let res = emit_invoke(b, ctx, window, args.len() + 1);
    let fallback = heap::unbox_dest(b, dest_ty, res)?;
    b.ins().jump(merge, &[fallback.into()]);

    b.switch_to_block(merge);
    Ok(Out::Native(b.block_params(merge)[0]))
}

/// The boxed `[callee, args…]` window on the native stack.
pub(super) fn boxed_window(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    callee: Value,
    args: &[u32],
) -> Result<Value, String> {
    let argc = args.len() + 1;
    let slot = b.create_sized_stack_slot(StackSlotData::new(
        StackSlotKind::ExplicitSlot,
        (argc * 16) as u32,
        4,
    ));
    let addr = b.ins().stack_addr(types::I64, slot, 0);
    b.ins().store(MemFlags::trusted(), callee, addr, 0);
    for (i, v) in args.iter().enumerate() {
        let boxed = heap::boxed_value(b, ctx, values, *v)?;
        b.ins()
            .store(MemFlags::trusted(), boxed, addr, ((i + 1) * 16) as i32);
    }
    Ok(addr)
}

/// Self-recursion out of a frame-aware body: a boxed window — this
/// activation's register 0 (the receiver of a method) then the arguments —
/// handed to `jit_call_self_window`, which runs a fresh activation of the
/// running closure. The boxed result.
pub(super) fn emit_self_call_framed(
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
    let receiver = super::use_heap(b, ctx, 0)?;
    let window = boxed_window(b, ctx, values, receiver, args)?;
    let argc = b.ins().iconst(types::I64, (args.len() + 1) as i64);
    call_helper_void(
        b,
        ctx.cc,
        ctx.helpers.jit_call_self_window,
        &[frame.exec_ctx, window, argc],
    );
    Ok(b.ins().load(
        types::I128,
        MemFlags::trusted(),
        frame.exec_ctx,
        ctx.helpers.jit_native_result_offset as i32,
    ))
}

/// `recv.name(args)`: `[receiver, args...]` boxed, handed to
/// `jit_call_method_window` with the method name's constant and the site's
/// cache slot. The boxed result.
pub(super) fn emit_method_call(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    recv: u32,
    name: &str,
    args: &[u32],
    cs: u16,
) -> Result<Value, String> {
    let frame = ctx
        .frame
        .as_ref()
        .ok_or("from_ssa: method call without a frame")?;
    let receiver = super::heap::boxed_value(b, ctx, values, recv)?;
    let window = boxed_window(b, ctx, values, receiver, args)?;
    let name_v = b
        .ins()
        .iconst(types::I64, super::props::str_idx(ctx, name)? as i64);
    let cs_v = b.ins().iconst(types::I64, i64::from(cs));
    let total = b.ins().iconst(types::I64, (args.len() + 1) as i64);
    call_helper_void(
        b,
        ctx.cc,
        ctx.helpers.jit_call_method_window,
        &[frame.exec_ctx, name_v, cs_v, window, total],
    );
    Ok(b.ins().load(
        types::I128,
        MemFlags::trusted(),
        frame.exec_ctx,
        ctx.helpers.jit_native_result_offset as i32,
    ))
}

/// A core-type native op: `[receiver, args...]` boxed, handed to
/// `jit_call_native_window` with the native resolved at compile time (or
/// `0`, resolved by op-id at the call). The boxed result.
pub(super) fn emit_call_native_op(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    object: u32,
    args: &[u32],
    op_id: u64,
) -> Result<Value, String> {
    let frame = ctx
        .frame
        .as_ref()
        .ok_or("from_ssa: native call without a frame")?;
    let receiver = super::heap::boxed_value(b, ctx, values, object)?;
    // `charCodeAt`/`codePointAt`: the dedicated helper, as the bytecode
    // lowering's unhoisted path — no argument window, no marshal.
    if args.len() == 1 && varn_core::op_id::is_str_char_index_op_id(op_id) {
        let (recv_tag, recv_payload) = b.ins().isplit(receiver);
        let (pos_tag, pos_payload) = super::heap::boxed_parts(b, ctx, values, args[0])?;
        let code = call_helper(
            b,
            ctx.cc,
            ctx.helpers.str_char_code_at,
            &[frame.exec_ctx, recv_tag, recv_payload, pos_tag, pos_payload],
        );
        return Ok(box_int(b, code));
    }
    let window = boxed_window(b, ctx, values, receiver, args)?;
    let target = (ctx.helpers.resolve_native_op)(op_id);
    let fn_v = b.ins().iconst(types::I64, target.func_ptr as i64);
    let op_v = b.ins().iconst(types::I64, op_id as i64);
    let total = b.ins().iconst(types::I64, (args.len() + 1) as i64);
    call_helper_void(
        b,
        ctx.cc,
        ctx.helpers.jit_call_native_window,
        &[frame.exec_ctx, fn_v, op_v, window, total],
    );
    Ok(b.ins().load(
        types::I128,
        MemFlags::trusted(),
        frame.exec_ctx,
        ctx.helpers.jit_native_result_offset as i32,
    ))
}

/// The canonical invocation of a boxed window (`ExecCtx::invoke`); the
/// result is the boxed `VmValue` the helper left in `jit_native_result`.
fn emit_invoke(b: &mut FunctionBuilder, ctx: &Ctx<'_>, window: Value, argc: usize) -> Value {
    let frame = ctx.frame.as_ref().expect("a call has a frame");
    let callee = b.ins().load(types::I128, MemFlags::trusted(), window, 0);
    let (ctag, cpayload) = b.ins().isplit(callee);
    let argc_v = b.ins().iconst(types::I64, argc as i64);
    call_helper_void(
        b,
        ctx.cc,
        ctx.helpers.jit_invoke_window,
        &[frame.exec_ctx, ctag, cpayload, window, argc_v],
    );
    b.ins().load(
        types::I128,
        MemFlags::trusted(),
        frame.exec_ctx,
        ctx.helpers.jit_native_result_offset as i32,
    )
}
