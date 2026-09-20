//! Cross-function calls, property access, and method invocations for CLIF allocation lowering.

use cranelift_codegen::ir::{condcodes::IntCC, types, InstBuilder, MemFlags};
use cranelift_frontend::FunctionBuilder;
use varn_types::register_meta::SlotKind;

use super::super::emit::{
    call_helper, call_helper_void, meta_is_float, unbox_bool, unbox_f64_coerce, use_f64, use_int,
};
use super::super::kinds::K;
use super::safepoints::{
    box_or_load_home, def_result, flush_boxed, live_boxed, reload_boxed,
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
    // Fase B: the inline class-construct fast path writes instance fields at a
    // 16-byte stride, wrong for the compact `InstanceData` layout. Route
    // `new X()` through the VM call window (compact-aware) until that path is
    // compact-aware too.
    let _ = class_target;
    let class_target: Option<&crate::clif::lower::ClifClassTarget> = None;

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
                let slow_res = emit_vm_call(b, actx, state, callee, arg_start, total, dest, ip + 3);
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
        let res = emit_vm_call(b, actx, state, callee, arg_start, total, dest, ip + 3);
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
    let boxed_slow = emit_vm_call(b, actx, state, callee, arg_start, total, dest, ip + 3);
    b.ins().jump(merge, &[boxed_slow.into()]);

    b.switch_to_block(merge);
    let res = b.block_params(merge)[0];
    def_result(b, actx, dest, res);
    Ok(())
}

/// Calls an arbitrary (possibly frame-aware) closure without crossing back
/// into the generic VM dispatch when the callee turns out to already have
/// compiled code. Two paths:
///
/// 1. `jit_prepare_static_call` pushes the callee activation and returns its
///    wrapper entry; the call site invokes the wrapper directly
///    ([`emit_wrapper_call_and_finish`]).
/// 2. `clif_call_fallback` — full generic VM dispatch
///    (`ExecCtx::call_vm_window`). Async/generator/rest closures, class
///    construction, native functions, or nothing compiled yet.
///
/// `dest`/`next_ip` feed the exception-unwind protocol
/// (`ExecCtx::jit_resume_ip`/`jit_call_dest`, see their docs in varn-vm):
/// written before every tier, so a caught throw below this call can resume
/// this (interpreted) caller from the right place.
#[allow(clippy::too_many_arguments)]
fn emit_vm_call(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    callee: cranelift_codegen::ir::Value,
    arg_start: usize,
    total: usize,
    dest: usize,
    next_ip: usize,
) -> cranelift_codegen::ir::Value {
    let (callee_tag, callee_payload) = b.ins().isplit(callee);

    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    // Every register's home must be current before the call: if the callee (or
    // anything below) throws and the exception is caught below this caller, the
    // compiled frame is abandoned and the interpreter RESUMES this frame reading
    // its registers out of their homes. `flush_boxed` only covers the heap
    // classes, so flush the GPR/FPR homes too (tests/65-safepoint-roots).
    for r in 0..actx.nregs {
        store_home(b, actx, state, r);
    }

    // Exception-unwind protocol: a throw below this call that is caught below
    // this (interpreted) caller resumes it from `next_ip`, and the callee's
    // return lands in `dest`. See `ExecCtx::jit_resume_ip`/`jit_call_dest`.
    let resume_ip_v = b.ins().iconst(types::I64, next_ip as i64);
    b.ins().store(
        MemFlags::trusted(),
        resume_ip_v,
        actx.exec_ctx,
        actx.helpers.jit_resume_ip_offset as i32,
    );
    let dest_v = b.ins().iconst(types::I64, dest as i64);
    b.ins().store(
        MemFlags::trusted(),
        dest_v,
        actx.exec_ctx,
        actx.helpers.jit_call_dest_offset as i32,
    );

    // One canonical VM call: `clif_call_fallback` gathers the argument window
    // from the caller's home slots and runs the callee through
    // `ExecCtx::call_vm_window` (prepare_call + run_until), which itself enters
    // the callee's compiled entry when one exists. The inline/wrapper tiers are
    // a later optimization; correctness first (Ley 8, one mechanism).
    let start_v = b.ins().iconst(types::I64, arg_start as i64);
    let n = b.ins().iconst(types::I64, total as i64);

    // Fast path: `jit_prepare_static_call` pushes the callee activation and
    // returns its compiled wrapper entry, so the call is direct
    // compiled→compiled. It declines (0) for a non-closure callee, an
    // async/generator/rest callee, or one with no compiled entry; the slow
    // path runs the whole call through the VM.
    let wrapper_addr = call_helper(
        b,
        actx.cc,
        actx.helpers.jit_prepare_static_call,
        &[
            actx.exec_ctx,
            callee_tag,
            callee_payload,
            actx.base,
            start_v,
            n,
        ],
    );
    let took_fast = b.ins().icmp_imm(IntCC::NotEqual, wrapper_addr, 0);
    let fast = b.create_block();
    let slow = b.create_block();
    let merge = b.create_block();
    b.append_block_param(merge, types::I128);
    b.ins().brif(took_fast, fast, &[], slow, &[]);

    b.switch_to_block(fast);
    let closure_ptr = b.ins().load(
        types::I64,
        MemFlags::trusted(),
        actx.exec_ctx,
        actx.helpers.jit_call_closure_ptr_offset as i32,
    );
    let callee_alloc = b.ins().load(
        types::I64,
        MemFlags::trusted(),
        actx.exec_ctx,
        actx.helpers.jit_call_base_offset as i32,
    );
    let fast_res =
        emit_wrapper_call_and_finish(b, actx, wrapper_addr, closure_ptr, callee_alloc);
    b.ins().jump(merge, &[fast_res.into()]);

    b.switch_to_block(slow);
    call_helper_void(
        b,
        actx.cc,
        actx.helpers.clif_call_fallback,
        &[
            actx.exec_ctx,
            callee_tag,
            callee_payload,
            actx.base,
            start_v,
            n,
        ],
    );
    let slow_res = b.ins().load(
        types::I128,
        MemFlags::trusted(),
        actx.exec_ctx,
        actx.helpers.jit_native_result_offset as i32,
    );
    b.ins().jump(merge, &[slow_res.into()]);

    b.switch_to_block(merge);
    let res = b.block_params(merge)[0];
    reload_boxed(b, actx, state, &regs);
    res
}

/// The `call_indirect` shared by tiers 1 and 2 of [`emit_vm_call`], plus the
/// cleanup after it: `jit_finish_static_call` pops the frame whichever tier
/// pushed, closes any upvalues captured out of it, and truncates the stack
/// window back down. That part stays a Rust call in both tiers — it walks
/// `ctx.open_upvalues` (a `Vec` this function does not hand-roll a push/pop
/// for) and is a no-op read-and-return the overwhelming majority of the
/// time (no upvalue was ever opened at or above `callee_base`), so it is a
/// cheap call even when it cannot be skipped outright.
fn emit_wrapper_call_and_finish(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    wrapper_addr: cranelift_codegen::ir::Value,
    closure_ptr: cranelift_codegen::ir::Value,
    callee_base: cranelift_codegen::ir::Value,
) -> cranelift_codegen::ir::Value {
    // The wrapper's first ABI word (`stack_ptr`) is unused under the
    // partitioned frame; pass a dummy so the arity is unchanged.
    let stack_ptr = b.ins().iconst(types::I64, 0);
    let is_windows = actx.cc == cranelift_codegen::isa::CallConv::WindowsFastcall;
    let wrapper_res = if is_windows {
        // Mirrors `build_wrapper`'s Windows struct-return convention exactly:
        // a caller-allocated 16-byte slot for (tag, payload), passed as the
        // first (special StructReturn) argument.
        let slot = b.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
            cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
            16,
            4,
        ));
        let sret = b.ins().stack_addr(types::I64, slot, 0);
        let mut sig = cranelift_codegen::ir::Signature::new(actx.cc);
        sig.params.push(cranelift_codegen::ir::AbiParam::special(
            types::I64,
            cranelift_codegen::ir::ArgumentPurpose::StructReturn,
        ));
        for _ in 0..4 {
            sig.params
                .push(cranelift_codegen::ir::AbiParam::new(types::I64));
        }
        let sig_ref = b.import_signature(sig);
        b.ins().call_indirect(
            sig_ref,
            wrapper_addr,
            &[sret, stack_ptr, closure_ptr, callee_base, actx.exec_ctx],
        );
        let tag = b.ins().load(types::I64, MemFlags::trusted(), sret, 0);
        let payload = b.ins().load(types::I64, MemFlags::trusted(), sret, 8);
        b.ins().iconcat(tag, payload)
    } else {
        let mut sig = cranelift_codegen::ir::Signature::new(actx.cc);
        for _ in 0..4 {
            sig.params
                .push(cranelift_codegen::ir::AbiParam::new(types::I64));
        }
        sig.returns
            .push(cranelift_codegen::ir::AbiParam::new(types::I64));
        sig.returns
            .push(cranelift_codegen::ir::AbiParam::new(types::I64));
        let sig_ref = b.import_signature(sig);
        let call = b.ins().call_indirect(
            sig_ref,
            wrapper_addr,
            &[stack_ptr, closure_ptr, callee_base, actx.exec_ctx],
        );
        let results = b.inst_results(call);
        let (tag, payload) = (results[0], results[1]);
        b.ins().iconcat(tag, payload)
    };
    call_helper_void(
        b,
        actx.cc,
        actx.helpers.jit_finish_static_call,
        &[actx.exec_ctx, callee_base],
    );
    wrapper_res
}

/// the recursive call goes through the helper that pushes a frame of its own.
pub(crate) fn emit_call_self(
    b: &mut FunctionBuilder,
    actx: &AllocCtx,
    state: &[K],
    arg_start: usize,
    total: usize,
) -> cranelift_codegen::ir::Value {
    let regs = live_boxed(actx, state);
    flush_boxed(b, actx, state, &regs);
    // Every register home current: a throw caught below resumes this frame
    // interpreted out of its homes.
    for r in 0..actx.nregs {
        store_home(b, actx, state, r);
    }
    let start_v = b.ins().iconst(types::I64, arg_start as i64);
    let n = b.ins().iconst(types::I64, total as i64);
    call_helper_void(
        b,
        actx.cc,
        actx.helpers.clif_call_self,
        &[actx.exec_ctx, actx.base, start_v, n],
    );
    reload_boxed(b, actx, state, &regs);
    b.ins().load(
        types::I128,
        MemFlags::trusted(),
        actx.exec_ctx,
        actx.helpers.jit_native_result_offset as i32,
    )
}

/// The field inline-cache probe shared by `GetProperty` / `SetProperty`.
/// `obj_tag` / `obj_payload` are the receiver's split VmValue. Handles two
/// shapes: a class `Instance` (an `INSTANCE_FIELD` entry keyed by `class_id`)
/// and a dynamic `Object` / `Record` (a `SHAPE_PROP` entry keyed by shape id).
/// On return the builder sits in a fresh block where `field_addr` (the
/// VmValue-slot address of the resolved field) and `is_nursery` (`i8`, non-zero
/// when the receiver is a nursery object — a store then needs no write barrier)
/// are valid. Anything else — non-heap, some other slot tag, cache miss, a hit
/// past the first four entries — jumps to `slow`.
fn emit_field_ic(
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

    const INSTANCE_FIELD: i64 = varn_types::chunk::ICKind::INSTANCE_FIELD as i64;
    const SHAPE_PROP: i64 = varn_types::chunk::ICKind::SHAPE_PROP as i64;

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

    // `keyed` converges the two receiver shapes: `key_id` is the class id or
    // the shape id; `key_kind` the matching `ICKind`; `values_base` the field
    // region start.
    let keyed = b.create_block();
    b.append_block_param(keyed, types::I64); // key_id
    b.append_block_param(keyed, types::I64); // key_kind
    b.append_block_param(keyed, types::I64); // values_base
    b.append_block_param(keyed, types::I64); // slot_bound (fields at slot >= this spilled)

    let tagb = b.ins().uload8(types::I64, m, slot_addr, 0);
    let is_inst = b
        .ins()
        .icmp_imm(IntCC::Equal, tagb, olay.instance_tag as i64);
    let inst_blk = b.create_block();
    let try_obj = b.create_block();
    b.ins().brif(is_inst, inst_blk, &[], try_obj, &[]);

    b.switch_to_block(inst_blk);
    {
        let obj_ptr = b
            .ins()
            .iadd_imm(slot_addr, olay.instance_payload_off as i64);
        let data_ptr = b.ins().load(types::I64, m, obj_ptr, 0);
        let cid = b
            .ins()
            .load(types::I32, m, data_ptr, olay.instance_class_id_off as i32);
        let cid = b.ins().uextend(types::I64, cid);
        let vbase = b.ins().iadd_imm(data_ptr, olay.instance_values_off as i64);
        let kind = b.ins().iconst(types::I64, INSTANCE_FIELD);
        // An instance's cached field slot is always valid — every declared
        // field is inline.
        let bound = b.ins().iconst(types::I64, i64::from(i32::MAX));
        b.ins().jump(
            keyed,
            &[cid.into(), kind.into(), vbase.into(), bound.into()],
        );
    }

    b.switch_to_block(try_obj);
    {
        let is_obj = b.ins().icmp_imm(IntCC::Equal, tagb, olay.object_tag as i64);
        let obj_blk = b.create_block();
        b.ins().brif(is_obj, obj_blk, &[], slow, &[]);
        b.switch_to_block(obj_blk);
        let obj_ptr = b.ins().iadd_imm(slot_addr, olay.payload_off as i64);
        let data_ptr = b.ins().load(types::I64, m, obj_ptr, 0);
        let shape_ptr = b.ins().load(types::I64, m, data_ptr, olay.shape_off as i32);
        let sid = b
            .ins()
            .load(types::I32, m, shape_ptr, olay.shape_id_off as i32);
        let sid = b.ins().uextend(types::I64, sid);
        let vbase = b.ins().iadd_imm(data_ptr, olay.values_off as i64);
        let kind = b.ins().iconst(types::I64, SHAPE_PROP);
        // Fields at slot >= `inline_len` spilled to the overflow store, which
        // the inline path cannot read.
        let ilen = b.ins().load(types::I32, m, data_ptr, olay.len_off as i32);
        let ilen = b.ins().uextend(types::I64, ilen);
        b.ins()
            .jump(keyed, &[sid.into(), kind.into(), vbase.into(), ilen.into()]);
    }

    b.switch_to_block(keyed);
    let key_id = b.block_params(keyed)[0];
    let key_kind = b.block_params(keyed)[1];
    let values_base = b.block_params(keyed)[2];
    let slot_bound = b.block_params(keyed)[3];

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
    b.append_block_param(resolved, types::I64); // field_addr
    b.append_block_param(resolved, types::I8); // is_nursery

    // Probe the first four entries (`CacheEntry` is 8 bytes: id u32 @0,
    // slot u16 @4, is_class u8 @6).
    for e in 0..4i32 {
        let eoff = e * 8;
        let eid = {
            let v = b.ins().load(types::I32, m, slot_base, eoff);
            b.ins().uextend(types::I64, v)
        };
        let eisc = b.ins().uload8(types::I64, m, slot_base, eoff + 6);
        let eslot = b.ins().uload16(types::I64, m, slot_base, eoff + 4);
        let id_ok = b.ins().icmp(IntCC::Equal, eid, key_id);
        let kind_ok = b.ins().icmp(IntCC::Equal, eisc, key_kind);
        let in_bound = b.ins().icmp(IntCC::UnsignedLessThan, eslot, slot_bound);
        let m1 = b.ins().band(id_ok, kind_ok);
        let hit = b.ins().band(m1, in_bound);
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

    // Fase B: the field inline-cache path resolves a byte offset that assumes
    // 16-byte instance fields; `InstanceData` is compact. Route every property
    // access through the runtime helper until the IC is compact-aware.
    let _ = emit_field_ic;
    b.ins().jump(slow, &[]);

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

    let obj = box_or_load_home(b, actx, state, obj_r);
    let val = box_or_load_home(b, actx, state, val_r);
    let (obj_tag, obj_payload) = b.ins().isplit(obj);
    let (val_tag, val_payload) = b.ins().isplit(val);

    let cont = b.create_block();
    let slow = b.create_block();

    // Fase B: same as `emit_get_property` — the IC byte offset assumes 16-byte
    // instance fields; route through the compact-aware helper.
    let _ = emit_field_ic;
    b.ins().jump(slow, &[]);

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
