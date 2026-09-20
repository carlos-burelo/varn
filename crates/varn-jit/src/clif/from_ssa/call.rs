//! Calls for the SSA lowering: direct self-recursion and cross-proto calls.
//!
//! `SelfCall` is handled in [`super::scalar`] (a leaf body's direct hardware
//! call). `Call` needs the frame (`closure` to read the callee global,
//! `exec_ctx` for the fallback): it resolves the callee against the linker by
//! the global slot it was loaded from, and lowers to a guarded direct call when
//! the target is a leaf with the same scalar signature; otherwise it falls back
//! to the canonical `ExecCtx::invoke` through `jit_invoke_window`. Call
//! arguments are restricted to scalars; a heap result is returned boxed for the
//! caller to land in its home.

use cranelift_codegen::ir::{
    condcodes::IntCC, types, AbiParam, InstBuilder, MemFlags, Signature, StackSlotData,
    StackSlotKind, Value,
};
use cranelift_frontend::FunctionBuilder;
use varn_types::register_meta::SlotKind;
use varn_types::vm_value::KIND_HEAP;

use super::{is_heap, load_value, Ctx};

use super::super::emit::{
    box_bool, box_f64, box_int, call_helper_void, unbox_bool, unbox_f64_coerce, unbox_int,
};

pub(super) fn emit_call(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    callee: u32,
    callee_global: Option<u32>,
    args: &[u32],
    dest: u32,
) -> Result<Value, String> {
    let frame = ctx
        .frame
        .as_ref()
        .ok_or("from_ssa: call in a frame-less body")?;
    let cc = ctx.cc;
    let helpers = ctx.helpers;

    let callee_v = load_value(b, ctx, values, callee)?;
    let arg_kinds: Vec<SlotKind> = args.iter().map(|v| ctx.ssa.value_ty(*v)).collect();
    for k in &arg_kinds {
        if !matches!(k, SlotKind::Int | SlotKind::Float | SlotKind::Bool) {
            return Err("from_ssa: non-scalar call argument".into());
        }
    }
    let arg_values: Vec<Value> = args
        .iter()
        .map(|v| load_value(b, ctx, values, *v))
        .collect::<Result<_, _>>()?;

    let slot = callee_global.ok_or("from_ssa: callee has no global provenance")?;
    let target = frame
        .linker
        .static_target(slot as usize)
        .ok_or("from_ssa: no static call target")?;
    if target.param_kinds.len() != arg_values.len()
        || target
            .param_kinds
            .iter()
            .zip(&arg_kinds)
            .any(|(want, got)| want != got)
    {
        return Err("from_ssa: call signature mismatch".into());
    }

    let dest_ty = ctx.ssa.value_ty(dest);
    let target_scalar_ret = matches!(
        target.return_kind,
        SlotKind::Int | SlotKind::Float | SlotKind::Bool
    );
    // The direct leaf path is scalar-only on both sides. A heap result, or a
    // callee whose declared return class is `Dynamic` (methods), always runs
    // the canonical fallback: it returns the boxed `VmValue`, which the caller
    // unboxes per `dest_ty` (or lands in the dest home).
    if is_heap(dest_ty) || !target_scalar_ret {
        return emit_fallback(
            b,
            cc,
            helpers,
            frame.exec_ctx,
            callee_v,
            &arg_values,
            &arg_kinds,
            dest_ty,
        );
    }

    let merge_ty = match dest_ty {
        SlotKind::Int | SlotKind::Bool => types::I64,
        SlotKind::Float => types::F64,
        _ => return Err("from_ssa: non-scalar call result".into()),
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
    let direct = {
        let mut sig = Signature::new(cc);
        for k in &target.param_kinds {
            sig.params.push(AbiParam::new(match k {
                SlotKind::Float => types::F64,
                _ => types::I64,
            }));
        }
        match target.return_kind {
            SlotKind::Int | SlotKind::Bool => sig.returns.push(AbiParam::new(types::I64)),
            SlotKind::Float => sig.returns.push(AbiParam::new(types::F64)),
            _ => return Err("from_ssa: non-scalar direct callee return".into()),
        }
        let sig_ref = b.import_signature(sig);
        let call = b.ins().call_indirect(sig_ref, raw, &arg_values);
        b.inst_results(call)[0]
    };
    b.ins().jump(merge, &[direct.into()]);

    b.switch_to_block(slow);
    let fallback = emit_fallback(
        b,
        cc,
        helpers,
        frame.exec_ctx,
        callee_v,
        &arg_values,
        &arg_kinds,
        dest_ty,
    )?;
    b.ins().jump(merge, &[fallback.into()]);

    b.switch_to_block(merge);
    Ok(b.block_params(merge)[0])
}

/// The frame-less canonical invocation: build the boxed `[callee, args…]`
/// window on the native stack and run it through `jit_invoke_window`
/// (`ExecCtx::invoke`), then unbox the result per its class (or return the
/// boxed value for a heap destination).
#[allow(clippy::too_many_arguments)]
fn emit_fallback(
    b: &mut FunctionBuilder,
    cc: cranelift_codegen::isa::CallConv,
    helpers: &crate::JitHelpers,
    exec_ctx: Value,
    callee: Value,
    args: &[Value],
    arg_kinds: &[SlotKind],
    dest: SlotKind,
) -> Result<Value, String> {
    let argc = args.len() + 1;
    let slot = b.create_sized_stack_slot(StackSlotData::new(
        StackSlotKind::ExplicitSlot,
        (argc * 16) as u32,
        4,
    ));
    let addr = b.ins().stack_addr(types::I64, slot, 0);
    b.ins().store(MemFlags::trusted(), callee, addr, 0);
    for (i, (av, k)) in args.iter().zip(arg_kinds).enumerate() {
        let boxed = match k {
            SlotKind::Int => box_int(b, *av),
            SlotKind::Float => box_f64(b, *av),
            SlotKind::Bool => box_bool(b, *av),
            _ => return Err("from_ssa: non-scalar call argument".into()),
        };
        b.ins()
            .store(MemFlags::trusted(), boxed, addr, ((i + 1) * 16) as i32);
    }
    let (ctag, cpayload) = b.ins().isplit(callee);
    let argc_v = b.ins().iconst(types::I64, argc as i64);
    call_helper_void(
        b,
        cc,
        helpers.jit_invoke_window,
        &[exec_ctx, ctag, cpayload, addr, argc_v],
    );
    let res = b.ins().load(
        types::I128,
        MemFlags::trusted(),
        exec_ctx,
        helpers.jit_native_result_offset as i32,
    );
    Ok(match dest {
        SlotKind::Int => unbox_int(b, res),
        SlotKind::Float => unbox_f64_coerce(b, res),
        SlotKind::Bool => unbox_bool(b, res),
        _ => res,
    })
}
