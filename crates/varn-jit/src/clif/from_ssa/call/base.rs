use cranelift_codegen::ir::{
    condcodes::IntCC, types, AbiParam, InstBuilder, MemFlags, Signature, Value,
};
use cranelift_frontend::FunctionBuilder;
use varn_types::register_meta::SlotKind;
use varn_types::vm_value::KIND_HEAP;

use super::super::super::emit::call_helper_void;
use super::super::{heap, load_value, Ctx, Out};
use super::invoke::{boxed_window, emit_invoke};

fn is_scalar(k: SlotKind) -> bool {
    matches!(k, SlotKind::Int | SlotKind::Float | SlotKind::Bool)
}

pub(crate) fn emit_call(
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
        let (ctag, cpayload) = b.ins().isplit(callee_v);
        return Ok(Out::Boxed(emit_invoke(
            b,
            ctx,
            window,
            (ctag, cpayload),
            args.len() + 1,
        )));
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
    let mut arg_values: Vec<Value> = args
        .iter()
        .map(|v| load_value(b, ctx, values, *v))
        .collect::<Result<_, _>>()?;
    arg_values.insert(0, ctx.exec_ctx);
    let direct = {
        let mut sig = Signature::new(ctx.cc);
        sig.params.push(AbiParam::new(types::I64));
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
    let res = emit_invoke(b, ctx, window, (ctag, cpayload), args.len() + 1);
    let fallback = heap::unbox_dest(b, dest_ty, res)?;
    super::super::store::drop_home_addrs(ctx);
    b.ins().jump(merge, &[fallback.into()]);

    b.switch_to_block(merge);
    Ok(Out::Native(b.block_params(merge)[0]))
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
    let receiver = super::super::use_heap(b, ctx, 0)?;
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
