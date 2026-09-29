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
use varn_types::ssa::SsaProto;
use varn_types::vm_value::KIND_HEAP;

use super::{heap, load_value, Ctx, Out};

use super::super::emit::{box_bool, box_int, call_helper, call_helper_void};

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
    // El callee leaf abre con exec_ctx: se antepone el propio.
    arg_values.insert(0, ctx.exec_ctx);
    let direct = {
        let mut sig = Signature::new(ctx.cc);
        sig.params.push(AbiParam::new(types::I64)); // exec_ctx
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
    // Addresses memoized while staging the window were computed inside this
    // arm, which does not dominate the merge (see `super::store`).
    super::store::drop_home_addrs(ctx);
    b.ins().jump(merge, &[fallback.into()]);

    b.switch_to_block(merge);
    Ok(Out::Native(b.block_params(merge)[0]))
}

/// One reusable native-stack window per function, sized to the largest
/// call/aggregate window the body needs (see [`scratch_max`]). Every helper
/// that takes a staged window reads it synchronously and copies what it
/// keeps into GC-visible staging first, so sequential uses never overlap:
/// one slot replaces one slot per call-site, and the frame shrinks by the
/// sum of the rest.
pub(crate) struct ScratchWin {
    addr: Value,
    max: usize,
}

impl ScratchWin {
    /// The shared window for `max` slots, or `None` when the body stages
    /// nothing. Emitted in the entry block, which dominates every use.
    pub(crate) fn create(b: &mut FunctionBuilder, max: usize) -> Option<ScratchWin> {
        if max == 0 {
            return None;
        }
        let slot = b.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            (max * 16) as u32,
            4,
        ));
        Some(ScratchWin {
            addr: b.ins().stack_addr(types::I64, slot, 0),
            max,
        })
    }
}

/// Largest window any instruction of `ssa` stages: call windows hold
/// `[callee, args…]`, aggregate windows their parts.
pub(super) fn scratch_max(ssa: &SsaProto) -> usize {
    use varn_types::ssa::SsaOp;
    let mut max = 0usize;
    for blk in &ssa.blocks {
        for inst in &blk.insts {
            let need = match &inst.op {
                SsaOp::Call { args, .. }
                | SsaOp::SelfCall { args }
                | SsaOp::SuperCall { args }
                | SsaOp::MethodCall { args, .. } => args.len() + 1,
                SsaOp::CallNativeOp { args, .. }
                | SsaOp::SuperMethodCall { args, .. }
                | SsaOp::ExtensionCall { args, .. } => args.len() + 1,
                SsaOp::CallSpread { args, .. } => args.len(),
                SsaOp::New { args, .. } => args.len(),
                SsaOp::IterCall { .. } => 2,
                SsaOp::Dispose { .. } => 1,
                SsaOp::IntrinsicCall { args, .. } => args.len() + 1,
                SsaOp::BuildStr { parts } | SsaOp::BuildTuple { elements: parts } => parts.len(),
                SsaOp::BuildArray { elements } => elements.len(),
                SsaOp::BuildMap { pairs } => pairs.len() * 2,
                SsaOp::ObjectRest { skip_keys, .. } => skip_keys.len(),
                _ => 0,
            };
            max = max.max(need);
        }
    }
    max
}

/// Address of a `count`-slot staged window: the shared one when it fits, a
/// private slot otherwise (defensive; the max above covers every op).
pub(super) fn scratch_addr(b: &mut FunctionBuilder, ctx: &Ctx<'_>, count: usize) -> Value {
    if let Some(s) = &ctx.scratch {
        if count <= s.max {
            return s.addr;
        }
    }
    let slot = b.create_sized_stack_slot(StackSlotData::new(
        StackSlotKind::ExplicitSlot,
        (count.max(1) * 16) as u32,
        4,
    ));
    b.ins().stack_addr(types::I64, slot, 0)
}

/// `new Class(...args)`: the class identity rides the callee value into a
/// dedicated helper that builds trivial instances inline (no constructor
/// frame) and runs anything else through the canonical construction both
/// tiers share. The boxed result.
pub(super) fn emit_new(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    callee: u32,
    _callee_global: Option<u32>,
    args: &[u32],
    _dest: Option<u32>,
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
        b.ins()
            .store(MemFlags::trusted(), *v, addr, (i * 16) as i32);
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
        MemFlags::trusted(),
        frame.exec_ctx,
        ctx.helpers.jit_native_result_offset as i32,
    )))
}

/// The boxed `[callee, args…]` window on the native stack.
pub(super) fn boxed_window(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    callee: Value,
    args: &[u32],
) -> Result<Value, String> {
    let addr = scratch_addr(b, ctx, args.len() + 1);
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

/// Branch to `yes` when boxed `tag`/`payload` is a string (inline SSO or a
/// heap `Str` slot), else to `no`. Read-only walk: no allocation, no calls,
/// so nothing it inspects can move under it.
#[allow(clippy::too_many_arguments)]
fn emit_is_str(
    b: &mut FunctionBuilder,
    ectx: Value,
    alay: &crate::JitArrayLayout,
    heap_off: usize,
    str_tag: usize,
    tag: Value,
    payload: Value,
    yes: cranelift_codegen::ir::Block,
    no: cranelift_codegen::ir::Block,
) {
    use cranelift_codegen::ir::{condcodes::IntCC, types, InstBuilder, MemFlags};
    let m = MemFlags::trusted();
    let k = b.ins().band_imm(tag, super::super::emit::KIND_MASK);
    let is_sso = b
        .ins()
        .icmp_imm(IntCC::Equal, k, varn_types::vm_value::KIND_SSO as i64);
    let is_heap = b
        .ins()
        .icmp_imm(IntCC::Equal, k, super::super::emit::HEAP_KIND);
    let walk = b.create_block();
    // SSO answers inline; non-heap non-SSO is never a string.
    let not_sso = b.create_block();
    b.ins().brif(is_sso, yes, &[], not_sso, &[]);
    b.switch_to_block(not_sso);
    b.ins().brif(is_heap, walk, &[], no, &[]);
    b.switch_to_block(walk);
    let raw = b.ins().band_imm(payload, 0xFFFF_FFFF);
    let rc = b.ins().load(types::I64, m, ectx, heap_off as i32);
    let old_bit = b.ins().band_imm(raw, 0x8000_0000);
    let base_old = b.ins().load(
        types::I64,
        m,
        rc,
        (alay.slots_vec_off + alay.slots_ptr_off) as i32,
    );
    let base_nur = b.ins().load(
        types::I64,
        m,
        rc,
        (alay.nursery_slots_vec_off + alay.slots_ptr_off) as i32,
    );
    let idx_old = b.ins().band_imm(raw, 0x7FFF_FFFF);
    let base = b.ins().select(old_bit, base_old, base_nur);
    let idx = b.ins().select(old_bit, idx_old, raw);
    let byte_off = b.ins().imul_imm(idx, alay.slot_size as i64);
    let slot_addr = b.ins().iadd(base, byte_off);
    let tagb = b.ins().uload8(types::I64, m, slot_addr, 0);
    let is_str = b.ins().icmp_imm(IntCC::Equal, tagb, str_tag as i64);
    b.ins().brif(is_str, yes, &[], no, &[]);
}
/// ecv.name(args)\: \[receiver, args...]\ boxed, handed to
/// \jit_call_method_window\ with the method name's constant and the site's
/// cache slot. The boxed result.
///
/// Instance fast lane: when the receiver is a heap `Instance` whose class id
/// matches a vtable entry (`NATIVE_VTABLE_METHOD`/`VM_VTABLE_METHOD`) of this
/// site's cache, the call goes to `jit_call_method_cached_window` with the
/// entry's class pointer — no name lookup, no registry, no string work. The
/// helper re-verifies version/slot and degrades to the canonical resolution
/// on anything unexpected, so the lane can only be fast, never wrong.
/// Anything else (non-instance receiver, cache miss) takes the generic
/// window helper, which owns the full semantics and populates the cache.
pub(super) fn emit_method_call(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    recv: u32,
    name: &str,
    args: &[u32],
    cs: u16,
) -> Result<Value, String> {
    use cranelift_codegen::ir::condcodes::IntCC;
    use cranelift_codegen::ir::{types, MemFlags};
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

    let slow = b.create_block();
    b.set_cold_block(slow);
    let merge = b.create_block();
    b.append_block_param(merge, types::I128);

    let m = MemFlags::trusted();
    let olay = &ctx.helpers.object_layout;
    let alay = &ctx.helpers.array_layout;
    let heap_off = ctx.helpers.heap_field_offset;
    let ectx = frame.exec_ctx;

    // 0. Core-string search lane: `startsWith`/`endsWith`/`indexOf` with
    // exactly one argument, receiver and argument both strings (inline or
    // heap). The op-id path (`CallNativeOp`) already steers statically-typed
    // receivers; this covers the dynamic ones (`headers["x"].startsWith`,
    // `url.indexOf` on a `dynamic`). Anything else — including a
    // user-defined method of the same name on another type — falls through
    // to the instance lane and the generic helper below, untouched.
    if args.len() == 1 && (name == "startsWith" || name == "endsWith" || name == "indexOf") {
        let arg0 = b.ins().load(types::I128, m, window, 16);
        let (at, ap) = b.ins().isplit(arg0);
        let (rt0, rp0) = b.ins().isplit(receiver);
        let inst_entry = b.create_block();
        let arg_check = b.create_block();
        let str_go = b.create_block();
        emit_is_str(
            b,
            ectx,
            alay,
            heap_off,
            ctx.helpers.str_layout.str_tag,
            rt0,
            rp0,
            arg_check,
            inst_entry,
        );
        b.switch_to_block(arg_check);
        emit_is_str(
            b,
            ectx,
            alay,
            heap_off,
            ctx.helpers.str_layout.str_tag,
            at,
            ap,
            str_go,
            inst_entry,
        );
        b.switch_to_block(str_go);
        let helper = if name == "startsWith" {
            ctx.helpers.str_starts_with
        } else if name == "endsWith" {
            ctx.helpers.str_ends_with
        } else {
            ctx.helpers.str_index_of
        };
        let r = call_helper(b, ctx.cc, helper, &[ectx, rt0, rp0, at, ap]);
        // `indexOf` answers `int`, the prefix tests answer 0/1.
        let boxed = if name == "indexOf" {
            box_int(b, r)
        } else {
            box_bool(b, r)
        };
        b.ins().jump(merge, &[boxed.into()]);
        b.switch_to_block(inst_entry);
    }

    // 1. Instance guard: heap tag + Instance slot tag, else generic.
    let (rt, rp) = b.ins().isplit(receiver);
    let kind = b.ins().band_imm(rt, super::super::emit::KIND_MASK);
    let is_heap = b
        .ins()
        .icmp_imm(IntCC::Equal, kind, super::super::emit::HEAP_KIND);
    let chk = b.create_block();
    b.ins().brif(is_heap, chk, &[], slow, &[]);
    b.switch_to_block(chk);
    let raw = b.ins().band_imm(rp, 0xFFFF_FFFF);
    let rc = b.ins().load(types::I64, m, ectx, heap_off as i32);
    let old_bit = b.ins().band_imm(raw, 0x8000_0000);
    let base_old = b.ins().load(
        types::I64,
        m,
        rc,
        (alay.slots_vec_off + alay.slots_ptr_off) as i32,
    );
    let base_nur = b.ins().load(
        types::I64,
        m,
        rc,
        (alay.nursery_slots_vec_off + alay.slots_ptr_off) as i32,
    );
    let idx_old = b.ins().band_imm(raw, 0x7FFF_FFFF);
    let base = b.ins().select(old_bit, base_old, base_nur);
    let idx = b.ins().select(old_bit, idx_old, raw);
    let byte_off = b.ins().imul_imm(idx, alay.slot_size as i64);
    let slot_addr = b.ins().iadd(base, byte_off);
    let tagb = b.ins().uload8(types::I64, m, slot_addr, 0);
    let is_inst = b
        .ins()
        .icmp_imm(IntCC::Equal, tagb, olay.instance_tag as i64);
    let ok = b.create_block();
    b.ins().brif(is_inst, ok, &[], slow, &[]);
    b.switch_to_block(ok);
    let data_ptr = b
        .ins()
        .load(types::I64, m, slot_addr, olay.instance_payload_off as i32);
    let cid32 = b
        .ins()
        .load(types::I32, m, data_ptr, olay.instance_class_id_off as i32);
    let cid = b.ins().uextend(types::I64, cid32);

    // 2. Scan the 8 entries for a live vtable hit on this class.
    let ic_base = b.ins().load(
        types::I64,
        m,
        frame.closure,
        ctx.helpers.closure_ic_entries_offset as i32,
    );
    let slot_base = b.ins().iadd_imm(
        ic_base,
        i64::from(cs) * (ctx.helpers.poly_ic_slot_size as i64),
    );
    let mut next = b.create_block();
    b.ins().jump(next, &[]);
    for i in 0..8 {
        b.switch_to_block(next);
        next = b.create_block();
        let hit = b.create_block();
        let entry = b.ins().iadd_imm(slot_base, (i * 8) as i64);
        let id32 = b.ins().load(types::I32, m, entry, 0);
        let id = b.ins().uextend(types::I64, id32);
        let kc = b.ins().uload8(types::I64, m, entry, 6);
        let id_eq = b.ins().icmp(IntCC::Equal, id, cid);
        let is_native = b.ins().icmp_imm(
            IntCC::Equal,
            kc,
            varn_types::chunk::ICKind::NATIVE_VTABLE_METHOD as i64,
        );
        let is_vm = b.ins().icmp_imm(
            IntCC::Equal,
            kc,
            varn_types::chunk::ICKind::VM_VTABLE_METHOD as i64,
        );
        let is_vtable = b.ins().bor(is_native, is_vm);
        let matched = b.ins().band(id_eq, is_vtable);
        b.ins().brif(matched, hit, &[], next, &[]);

        b.switch_to_block(hit);
        // Vtable entries always carry their class (recorded with `Some`);
        // a null pointer means this entry is not what it claims — miss on.
        let classp = b.ins().load(types::I64, m, entry, 8);
        let has_class = b.ins().icmp_imm(IntCC::NotEqual, classp, 0);
        let go = b.create_block();
        b.ins().brif(has_class, go, &[], next, &[]);
        b.switch_to_block(go);
        let slot16 = b.ins().uload16(types::I64, m, entry, 4);
        let ver8 = b.ins().uload8(types::I64, m, entry, 7);
        call_helper_void(
            b,
            ctx.cc,
            ctx.helpers.jit_call_method_cached_window,
            &[
                ectx, classp, id, slot16, kc, ver8, name_v, cs_v, window, total,
            ],
        );
        super::store::drop_home_addrs(ctx);
        let hit_res = b.ins().load(
            types::I128,
            MemFlags::trusted(),
            ectx,
            ctx.helpers.jit_native_result_offset as i32,
        );
        super::store::drop_home_addrs(ctx);
        b.ins().jump(merge, &[hit_res.into()]);
    }
    b.switch_to_block(next);
    b.ins().jump(slow, &[]);

    b.switch_to_block(slow);
    call_helper_void(
        b,
        ctx.cc,
        ctx.helpers.jit_call_method_window,
        &[frame.exec_ctx, name_v, cs_v, window, total],
    );
    super::store::drop_home_addrs(ctx);
    let slow_res = b.ins().load(
        types::I128,
        MemFlags::trusted(),
        ectx,
        ctx.helpers.jit_native_result_offset as i32,
    );
    super::store::drop_home_addrs(ctx);
    b.ins().jump(merge, &[slow_res.into()]);

    b.switch_to_block(merge);
    Ok(b.block_params(merge)[0])
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
    // `startsWith`/`endsWith`/`indexOf` on a proven `str`: the checker proved
    // receiver and argument are strings, so these dedicated helpers run the
    // contract's own bodies with no window, no staging, no generic dispatch.
    if args.len() == 1 {
        let (recv_tag, recv_payload) = b.ins().isplit(receiver);
        let (arg_tag, arg_payload) = super::heap::boxed_parts(b, ctx, values, args[0])?;
        if op_id == varn_core::op_id::str_starts_with_op_id() {
            let r = call_helper(
                b,
                ctx.cc,
                ctx.helpers.str_starts_with,
                &[frame.exec_ctx, recv_tag, recv_payload, arg_tag, arg_payload],
            );
            return Ok(box_bool(b, r));
        }
        if op_id == varn_core::op_id::str_ends_with_op_id() {
            let r = call_helper(
                b,
                ctx.cc,
                ctx.helpers.str_ends_with,
                &[frame.exec_ctx, recv_tag, recv_payload, arg_tag, arg_payload],
            );
            return Ok(box_bool(b, r));
        }
        if op_id == varn_core::op_id::str_index_of_op_id() {
            let r = call_helper(
                b,
                ctx.cc,
                ctx.helpers.str_index_of,
                &[frame.exec_ctx, recv_tag, recv_payload, arg_tag, arg_payload],
            );
            return Ok(box_int(b, r));
        }
    }
    // `split(separator?)`: 0 or 1 argument, array result in
    // `jit_native_result`. Same contract body as the native, no window.
    if op_id == varn_core::op_id::str_split_op_id() && args.len() <= 1 {
        let (recv_tag, recv_payload) = b.ins().isplit(receiver);
        let argc_v = b.ins().iconst(types::I64, args.len() as i64);
        let (sep_tag, sep_payload) = if args.len() == 1 {
            super::heap::boxed_parts(b, ctx, values, args[0])?
        } else {
            (b.ins().iconst(types::I64, 0), b.ins().iconst(types::I64, 0))
        };
        call_helper_void(
            b,
            ctx.cc,
            ctx.helpers.str_split,
            &[
                frame.exec_ctx,
                recv_tag,
                recv_payload,
                argc_v,
                sep_tag,
                sep_payload,
            ],
        );
        return Ok(b.ins().load(
            types::I128,
            MemFlags::trusted(),
            frame.exec_ctx,
            ctx.helpers.jit_native_result_offset as i32,
        ));
    }
    // Native `slice(start, end?)`: 1 or 2 int arguments. The single-index
    // `str_slice` helper serves a different (`StrSlice` opcode) semantics, so
    // this steers to the range helper with the contract's body.
    if op_id == varn_core::op_id::str_slice_op_id() && (args.len() == 1 || args.len() == 2) {
        let (recv_tag, recv_payload) = b.ins().isplit(receiver);
        let (start_tag, start_payload) = super::heap::boxed_parts(b, ctx, values, args[0])?;
        let has_end_v = b.ins().iconst(types::I64, (args.len() - 1) as i64);
        let (end_tag, end_payload) = if args.len() == 2 {
            super::heap::boxed_parts(b, ctx, values, args[1])?
        } else {
            (b.ins().iconst(types::I64, 0), b.ins().iconst(types::I64, 0))
        };
        call_helper_void(
            b,
            ctx.cc,
            ctx.helpers.str_slice_range,
            &[
                frame.exec_ctx,
                recv_tag,
                recv_payload,
                start_tag,
                start_payload,
                has_end_v,
                end_tag,
                end_payload,
            ],
        );
        return Ok(b.ins().load(
            types::I128,
            MemFlags::trusted(),
            frame.exec_ctx,
            ctx.helpers.jit_native_result_offset as i32,
        ));
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
/// Takes the callee pre-split: every caller already holds it boxed and
/// splitting twice was pure waste.
pub(super) fn emit_invoke(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    window: Value,
    callee: (Value, Value),
    argc: usize,
) -> Value {
    let frame = ctx.frame.as_ref().expect("a call has a frame");
    let (ctag, cpayload) = callee;
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
