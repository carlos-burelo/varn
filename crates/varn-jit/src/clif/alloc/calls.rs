//! Cross-function calls, property access, and method invocations for CLIF allocation lowering.

use cranelift_codegen::ir::{condcodes::IntCC, types, InstBuilder, MemFlags};
use cranelift_frontend::FunctionBuilder;
use varn_types::register_meta::SlotKind;

use super::super::emit::{
    call_helper_void, meta_is_float, unbox_bool, unbox_f64_coerce, use_f64, use_int,
};
use super::super::kinds::K;
use super::safepoints::{
    box_or_load_home, def_result, flush_boxed, frame_base_addr, live_boxed, reload_boxed,
    store_home, AllocCtx,
};

pub(crate) fn emit_call(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
    target: Option<&crate::clif::lower::ClifTarget>,
    class_target: Option<&crate::clif::lower::ClifClassTarget>,
) -> Result<(), String> {
    let w1 = code[ip + 1];
    let w2 = code[ip + 2];
    let dest = (w1 >> 8) as usize;
    let callee_reg = (w1 & 0xFF) as usize;
    let total = (w2 >> 8) as usize;
    let arg_start = (w2 & 0xFF) as usize;

    let callee = box_or_load_home(b, actx, state, callee_reg);

    if let Some(ct) = class_target {
        if let Some(ref plan) = ct.trivial_plan {
            let valid_plan = plan.iter().all(|&(param_idx, _)| {
                1 + param_idx < total && arg_start + 1 + param_idx < actx.nregs
            });
            if valid_plan && arg_start + total <= actx.nregs {
                let fast_blk = b.create_block();
                let slow_blk = b.create_block();
                let cont_blk = b.create_block();

                let (callee_tag, callee_payload) = b.ins().isplit(callee);
                let expected_tag = b
                    .ins()
                    .iconst(types::I64, varn_types::vm_value::KIND_HEAP as i64);
                let expected_payload = b.ins().iconst(types::I64, ct.expected_bits as i64);
                let tag_matches = b.ins().icmp(IntCC::Equal, callee_tag, expected_tag);
                let payload_matches = b.ins().icmp(IntCC::Equal, callee_payload, expected_payload);
                let callee_ok = b.ins().band(tag_matches, payload_matches);
                b.ins().brif(callee_ok, fast_blk, &[], slow_blk, &[]);

                b.switch_to_block(fast_blk);
                let cid_val = b.ins().iconst(types::I64, ct.class_id as i64);
                let psize_val = b.ins().iconst(types::I64, ct.payload_size as i64);
                let raw_idx = super::super::emit::call_helper(
                    b,
                    actx.cc,
                    actx.helpers.alloc_instance_fast,
                    &[actx.exec_ctx, cid_val, psize_val],
                );
                let inst_tag = b
                    .ins()
                    .iconst(types::I64, varn_types::vm_value::KIND_HEAP as i64);
                let instance_nv = b.ins().iconcat(inst_tag, raw_idx);

                let data_base = super::super::emit::emit_object_data_base(
                    b,
                    actx.exec_ctx,
                    instance_nv,
                    &actx.helpers.object_layout,
                    &actx.helpers.array_layout,
                    actx.helpers.heap_field_offset,
                    slow_blk,
                );

                for &(param_idx, slot) in plan {
                    let arg_r = arg_start + 1 + param_idx;
                    let val = box_or_load_home(b, actx, state, arg_r);
                    let val128 = if b.func.dfg.value_type(val) == types::I128 {
                        val
                    } else if b.func.dfg.value_type(val) == types::F64 {
                        super::super::emit::box_f64(b, val)
                    } else {
                        match state.get(arg_r).copied().unwrap_or(K::Unset) {
                            K::Int => super::super::emit::box_int(b, val),
                            K::Bool => super::super::emit::box_bool(b, val),
                            K::Float => {
                                let f = b.ins().bitcast(types::F64, MemFlags::new(), val);
                                super::super::emit::box_f64(b, f)
                            }
                            _ => {
                                let tag_v = b
                                    .ins()
                                    .iconst(types::I64, varn_types::vm_value::KIND_HEAP as i64);
                                b.ins().iconcat(tag_v, val)
                            }
                        }
                    };
                    let slot_off = (slot * 16) as i32;
                    b.ins()
                        .store(MemFlags::trusted(), val128, data_base, slot_off);
                }

                def_result(b, actx, dest, instance_nv);
                b.ins().jump(cont_blk, &[]);

                b.switch_to_block(slow_blk);
                let slow_res = emit_vm_call(b, actx, state, callee, arg_start, total);
                def_result(b, actx, dest, slow_res);
                b.ins().jump(cont_blk, &[]);

                b.switch_to_block(cont_blk);
                return Ok(());
            }
        }
    }

    let direct = target.filter(|t| {
        t.raw_slot != 0
            && t.param_kinds.len() + 1 == total
            && arg_start + total <= actx.nregs
            && t.param_kinds.iter().enumerate().all(|(i, k)| {
                let r = arg_start + 1 + i;
                if *k != SlotKind::Int {
                    return true;
                }
                let unboxable = state[r] == K::Int || super::super::kinds::is_boxed_kind(state[r]);
                unboxable && !meta_is_float(actx.register_meta, r)
            })
    });

    let Some(t) = direct else {
        let res = emit_vm_call(b, actx, state, callee, arg_start, total);
        def_result(b, actx, dest, res);
        return Ok(());
    };

    let mut raw_args = Vec::with_capacity(t.param_kinds.len());
    for (i, k) in t.param_kinds.iter().enumerate() {
        let r = arg_start + 1 + i;
        let v = if *k == SlotKind::Int {
            use_int(b, actx.vars, state, r)?
        } else if *k == SlotKind::Float {
            if meta_is_float(actx.register_meta, r) || state[r] == K::Float {
                use_f64(b, actx.vars, state, r)?
            } else {
                let boxed = box_or_load_home(b, actx, state, r);
                unbox_f64_coerce(b, boxed)
            }
        } else if *k == SlotKind::Bool {
            let boxed = box_or_load_home(b, actx, state, r);
            unbox_bool(b, boxed)
        } else {
            let boxed = box_or_load_home(b, actx, state, r);
            let (_tag, payload) = b.ins().isplit(boxed);
            payload
        };
        raw_args.push(v);
    }

    let (callee_tag, callee_payload) = b.ins().isplit(callee);
    let expected_tag = b
        .ins()
        .iconst(types::I64, varn_types::vm_value::KIND_HEAP as i64);
    let same_tag = b.ins().icmp(IntCC::Equal, callee_tag, expected_tag);
    let expected_payload = b.ins().iconst(types::I64, t.expected_bits as i64);
    let same_payload = b.ins().icmp(IntCC::Equal, callee_payload, expected_payload);
    let same = b.ins().band(same_tag, same_payload);
    let slot = b.ins().iconst(types::I64, t.raw_slot as i64);
    let raw = b.ins().load(types::I64, MemFlags::trusted(), slot, 0);
    let published = b.ins().icmp_imm(IntCC::NotEqual, raw, 0);
    let take_direct = b.ins().band(same, published);

    let fast = b.create_block();
    let slow = b.create_block();
    let merge = b.create_block();
    b.append_block_param(merge, types::I128);
    b.ins().brif(take_direct, fast, &[], slow, &[]);

    b.switch_to_block(fast);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    let raw_sig = {
        let mut s = cranelift_codegen::ir::Signature::new(actx.cc);
        for k in &t.param_kinds {
            if *k == SlotKind::Float {
                s.params
                    .push(cranelift_codegen::ir::AbiParam::new(types::F64));
            } else {
                s.params
                    .push(cranelift_codegen::ir::AbiParam::new(types::I64));
            }
        }
        if t.return_kind == SlotKind::Int || t.return_kind == SlotKind::Bool {
            s.returns
                .push(cranelift_codegen::ir::AbiParam::new(types::I64));
        } else if t.return_kind == SlotKind::Float {
            s.returns
                .push(cranelift_codegen::ir::AbiParam::new(types::F64));
        }
        b.import_signature(s)
    };
    let boxed_fast = if t.return_kind == SlotKind::Int
        || t.return_kind == SlotKind::Bool
        || t.return_kind == SlotKind::Float
    {
        let call = b.ins().call_indirect(raw_sig, raw, &raw_args);
        let raw_res = b.inst_results(call)[0];
        super::super::emit::retag_raw_return(b, raw_res, t.return_kind)
    } else {
        b.ins().call_indirect(raw_sig, raw, &raw_args);
        b.ins().load(
            types::I128,
            MemFlags::trusted(),
            actx.exec_ctx,
            actx.helpers.jit_native_result_offset as i32,
        )
    };
    reload_boxed(b, actx, state, &regs);
    b.ins().jump(merge, &[boxed_fast.into()]);

    b.switch_to_block(slow);
    let boxed_slow = emit_vm_call(b, actx, state, callee, arg_start, total);
    b.ins().jump(merge, &[boxed_slow.into()]);

    b.switch_to_block(merge);
    let res = b.block_params(merge)[0];
    def_result(b, actx, dest, res);
    Ok(())
}

/// Hands `total` registers starting at `arg_start` to a VM-side call helper as
/// a stack window, and reads the boxed result back out of `jit_native_result`.
///
/// `helper` is called as `(exec_ctx, ..callee, src, argc)`; `callee` is what
/// distinguishes the variants, and everything around it — the safepoint flush,
/// writing the argument window to its home slots, the reload — is identical and
/// belongs in one place.
fn emit_helper_call_window(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    helper: usize,
    callee: &[cranelift_codegen::ir::Value],
    arg_start: usize,
    total: usize,
) -> cranelift_codegen::ir::Value {
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    let fb = frame_base_addr(b, actx);
    for r in arg_start..(arg_start + total).min(actx.nregs) {
        store_home(b, actx, state, fb, r);
    }

    let src = b.ins().iadd_imm(actx.base, arg_start as i64);
    let n = b.ins().iconst(types::I64, total as i64);
    let mut args = Vec::with_capacity(3 + callee.len());
    args.push(actx.exec_ctx);
    args.extend_from_slice(callee);
    args.push(src);
    args.push(n);
    call_helper_void(b, actx.cc, helper, &args);
    reload_boxed(b, actx, state, &regs);
    b.ins().load(
        types::I128,
        MemFlags::trusted(),
        actx.exec_ctx,
        actx.helpers.jit_native_result_offset as i32,
    )
}

fn emit_vm_call(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    callee: cranelift_codegen::ir::Value,
    arg_start: usize,
    total: usize,
) -> cranelift_codegen::ir::Value {
    let (callee_tag, callee_payload) = b.ins().isplit(callee);
    emit_helper_call_window(
        b,
        actx,
        state,
        actx.helpers.clif_call_fallback,
        &[callee_tag, callee_payload],
        arg_start,
        total,
    )
}

/// Direct self-recursion. A frame-aware lowering cannot pass its own `base` to
/// the callee — the callee would write its home slots over the caller's — so
/// the recursive call goes through the helper that pushes a frame of its own.
pub(crate) fn emit_call_self(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    arg_start: usize,
    total: usize,
) -> cranelift_codegen::ir::Value {
    emit_helper_call_window(
        b,
        actx,
        state,
        actx.helpers.clif_call_self,
        &[],
        arg_start,
        total,
    )
}

/// The instance-field inline-cache probe shared by `GetProperty` /
/// `SetProperty`. `obj_tag` / `obj_payload` are the receiver's split VmValue.
/// On return the builder sits in a fresh block where `field_addr` (the
/// VmValue-slot address of the resolved field) and `is_nursery` (`i8`, non-zero
/// when the receiver is a nursery object — a store then needs no write barrier)
/// are valid. Every shape the probe does not handle — non-heap, non-instance,
/// cache miss, a hit past the first four entries, any non-`INSTANCE_FIELD`
/// kind — jumps to `slow`.
fn emit_instance_field_ic(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    obj_tag: cranelift_codegen::ir::Value,
    obj_payload: cranelift_codegen::ir::Value,
    cs_idx: usize,
    slow: cranelift_codegen::ir::Block,
) -> (cranelift_codegen::ir::Value, cranelift_codegen::ir::Value) {
    use super::super::emit::{HEAP_KIND, KIND_MASK};
    let m = MemFlags::trusted();
    let olay = actx.helpers.object_layout;
    let alay = actx.helpers.array_layout;

    let tag = b.ins().band_imm(obj_tag, KIND_MASK);
    let is_heap = b.ins().icmp_imm(IntCC::Equal, tag, HEAP_KIND);
    let probe = b.create_block();
    b.ins().brif(is_heap, probe, &[], slow, &[]);
    b.switch_to_block(probe);

    // heap index → slot address (generation bit picks old-gen vs nursery vec)
    let raw = b.ins().band_imm(obj_payload, 0xFFFF_FFFF);
    let rc = b.ins().load(
        types::I64,
        m,
        actx.exec_ctx,
        actx.helpers.heap_field_offset as i32,
    );
    let old_bit = b.ins().band_imm(raw, 0x8000_0000);
    let is_nursery = b.ins().icmp_imm(IntCC::Equal, old_bit, 0);
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
    let sbase = b.ins().select(old_bit, base_old, base_nur);
    let sidx = b.ins().select(old_bit, idx_old, raw);
    let byte_off = b.ins().imul_imm(sidx, alay.slot_size as i64);
    let slot_addr = b.ins().iadd(sbase, byte_off);

    // Must be `HeapObj::Instance` — the only shape whose `class_id` lives at a
    // fixed header offset and whose `INSTANCE_FIELD` cache entries are valid.
    let tagb = b.ins().uload8(types::I64, m, slot_addr, 0);
    let is_inst = b
        .ins()
        .icmp_imm(IntCC::Equal, tagb, olay.instance_tag as i64);
    let inst_ok = b.create_block();
    b.ins().brif(is_inst, inst_ok, &[], slow, &[]);
    b.switch_to_block(inst_ok);

    let obj_ptr = b
        .ins()
        .iadd_imm(slot_addr, olay.instance_payload_off as i64);
    let data_ptr = b.ins().load(types::I64, m, obj_ptr, 0);
    let class_id = {
        let cid = b
            .ins()
            .load(types::I32, m, data_ptr, olay.instance_class_id_off as i32);
        b.ins().uextend(types::I64, cid)
    };
    let values_base = b.ins().iadd_imm(data_ptr, olay.instance_values_off as i64);

    // Poly slot for this call site: `ic_entries + cs * poly_ic_slot_size`.
    let ic_base = b.ins().load(
        types::I64,
        m,
        actx.closure,
        actx.helpers.closure_ic_entries_offset as i32,
    );
    let slot_base = b
        .ins()
        .iadd_imm(ic_base, (cs_idx * actx.helpers.poly_ic_slot_size) as i64);

    let resolved = b.create_block();
    b.append_block_param(resolved, types::I64);
    b.append_block_param(resolved, types::I8); // is_nursery (icmp result)

    // Probe the first four entries (`CacheEntry` is 8 bytes: id u32 @0,
    // slot u16 @4, is_class u8 @6).
    const INSTANCE_FIELD: i64 = varn_types::chunk::ICKind::INSTANCE_FIELD as i64;
    for e in 0..4i32 {
        let eoff = e * 8;
        let eid = {
            let v = b.ins().load(types::I32, m, slot_base, eoff);
            b.ins().uextend(types::I64, v)
        };
        let eisc = b.ins().uload8(types::I64, m, slot_base, eoff + 6);
        let eslot = b.ins().uload16(types::I64, m, slot_base, eoff + 4);
        let id_ok = b.ins().icmp(IntCC::Equal, eid, class_id);
        let kind_ok = b.ins().icmp_imm(IntCC::Equal, eisc, INSTANCE_FIELD);
        let hit = b.ins().band(id_ok, kind_ok);
        let do_hit = b.create_block();
        let next = b.create_block();
        b.ins().brif(hit, do_hit, &[], next, &[]);

        b.switch_to_block(do_hit);
        let field_off = b.ins().imul_imm(eslot, 16);
        let field_addr = b.ins().iadd(values_base, field_off);
        b.ins()
            .jump(resolved, &[field_addr.into(), is_nursery.into()]);

        b.switch_to_block(next);
    }
    b.ins().jump(slow, &[]);

    b.switch_to_block(resolved);
    let field_addr = b.block_params(resolved)[0];
    let is_nursery = b.block_params(resolved)[1];
    (field_addr, is_nursery)
}

pub(crate) fn emit_get_property(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    _meta: &[varn_types::register_meta::RegisterMeta],
    code: &[u16],
    ip: usize,
) {
    let dest = (code[ip] >> 8) as usize;
    let obj_r = (code[ip + 1] >> 8) as usize;
    let cs_idx = (code[ip + 1] & 0xFF) as usize;
    let name_idx = code[ip + 2] as usize;
    let next_ip = ip + 3;

    let m = MemFlags::trusted();
    let obj = box_or_load_home(b, actx, state, obj_r);
    let (obj_tag, obj_payload) = b.ins().isplit(obj);

    let cont = b.create_block();
    b.append_block_param(cont, types::I128);
    let slow = b.create_block();

    let (field_addr, _is_nursery) =
        emit_instance_field_ic(b, actx, obj_tag, obj_payload, cs_idx, slow);
    let v = b.ins().load(types::I128, m, field_addr, 0);
    b.ins().jump(cont, &[v.into()]);

    // ── Slow path: the runtime property helper ──
    b.switch_to_block(slow);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);

    let ni = b.ins().iconst(types::I64, name_idx as i64);
    let ci = b.ins().iconst(types::I64, cs_idx as i64);
    let de = b.ins().iconst(types::I64, dest as i64);
    let ipv = b.ins().iconst(types::I64, next_ip as i64);
    call_helper_void(
        b,
        actx.cc,
        actx.helpers.get_property_flat,
        &[
            actx.exec_ctx,
            actx.closure,
            actx.base,
            obj_tag,
            obj_payload,
            ni,
            ci,
            de,
            ipv,
        ],
    );
    reload_boxed(b, actx, state, &regs);
    let res = b.ins().load(
        types::I128,
        m,
        actx.exec_ctx,
        actx.helpers.jit_native_result_offset as i32,
    );
    b.ins().jump(cont, &[res.into()]);

    b.switch_to_block(cont);
    let v = b.block_params(cont)[0];
    def_result(b, actx, dest, v);
}

pub(crate) fn emit_set_property(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
) {
    let obj_r = (code[ip] >> 8) as usize;
    let val_r = (code[ip + 1] >> 8) as usize;
    let cs_idx = (code[ip + 1] & 0xFF) as usize;
    let name_idx = code[ip + 2] as usize;
    let next_ip = ip + 3;

    let m = MemFlags::trusted();
    let obj = box_or_load_home(b, actx, state, obj_r);
    let val = box_or_load_home(b, actx, state, val_r);
    let (obj_tag, obj_payload) = b.ins().isplit(obj);
    let (val_tag, val_payload) = b.ins().isplit(val);

    let cont = b.create_block();
    let slow = b.create_block();

    // A nursery instance field the checker did not pin: store the VmValue
    // directly. An old-gen receiver falls to the helper, which carries the
    // old←young write barrier.
    let (field_addr, is_nursery) =
        emit_instance_field_ic(b, actx, obj_tag, obj_payload, cs_idx, slow);
    let inline_store = b.create_block();
    b.ins().brif(is_nursery, inline_store, &[], slow, &[]);
    b.switch_to_block(inline_store);
    let val128 = b.ins().iconcat(val_tag, val_payload);
    b.ins().store(m, val128, field_addr, 0);
    b.ins().jump(cont, &[]);

    // ── Slow path: the runtime property helper ──
    b.switch_to_block(slow);
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);

    let ni = b.ins().iconst(types::I64, name_idx as i64);
    let ci = b.ins().iconst(types::I64, cs_idx as i64);
    let ipv = b.ins().iconst(types::I64, next_ip as i64);
    call_helper_void(
        b,
        actx.cc,
        actx.helpers.set_property_flat,
        &[
            actx.exec_ctx,
            actx.closure,
            obj_tag,
            obj_payload,
            val_tag,
            val_payload,
            ni,
            ci,
            ipv,
        ],
    );
    reload_boxed(b, actx, state, &regs);
    b.ins().jump(cont, &[]);

    b.switch_to_block(cont);
}
