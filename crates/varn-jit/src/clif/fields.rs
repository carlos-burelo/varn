//! Object fixed-field access lowering for CLIF: `GetFixedField` /
//! `SetFixedField`. The slot is known from the bytecode and the opcode is only
//! emitted for a class-typed receiver whose shape is proven, so access lowers
//! to an inline machine load/store: resolve the boxed receiver to its heap
//! slot, guard object-tag + inline-length, then a constant-offset field
//! access. Any guard miss (a non-object, an overflowed slot past the inline
//! tail, or — for the store — an old-gen receiver that needs the write
//! barrier) bails to the runtime helper. Ports the template's inline fast path
//! (`codegen/misc/objects.rs`).

use cranelift_codegen::ir::{condcodes::IntCC, types, InstBuilder, MemFlags, Value};
use cranelift_codegen::isa::CallConv;
use cranelift_frontend::{FunctionBuilder, Variable};
use varn_types::register_meta::RegisterMeta;

use super::alloc::AllocCtx;
use super::emit::{
    self, box_or_pass, call_helper_void, meta_is_float, state_meta_int, unbox_f64_coerce,
    unbox_int, use_boxed, HEAP_KIND,
};
use super::kinds::K;
use crate::JitHelpers;

/// Shared context for the fixed-field arms.
pub(crate) struct FldCtx<'a> {
    pub vars: &'a [Variable],
    pub helpers: &'a JitHelpers,
    pub cc: CallConv,
    pub exec_ctx: cranelift_codegen::ir::Value,
    pub register_meta: &'a [RegisterMeta],
    pub loop_caches: emit::LoopCaches<'a>,
    pub local_obj_bases: &'a rustc_hash::FxHashMap<usize, Variable>,
}

/// Resolve boxed object `obj` + `slot` to the inline field's machine address,
/// branching to `slow` on any guard miss (non-heap, non-object, or a slot past
/// the inline tail). `nursery_only` bails an old-gen receiver to `slow` too —
/// the store uses it so the helper carries the old←young write barrier. On
/// return the builder sits in a block where the returned address is valid.
fn emit_object_field_addr(
    b: &mut FunctionBuilder,
    c: &FldCtx,
    obj: Value,
    slot: usize,
    slow: cranelift_codegen::ir::Block,
    nursery_only: bool,
) -> Value {
    let alay = &c.helpers.array_layout;
    let olay = &c.helpers.object_layout;
    let heap_off = c.helpers.heap_field_offset;
    // Plain trusted (not readonly/movable): the loads dereference pointers the
    // guards above just validated, so the mid-end must not hoist them past
    // those guards. Loop-invariant hoisting of the resolve is a future lever.
    let m = MemFlags::trusted();

    // 1. Heap-pointer tag check.
    let (tag, raw_payload) = if b.func.dfg.value_type(obj) == types::I128 {
        b.ins().isplit(obj)
    } else {
        (b.ins().iconst(types::I64, HEAP_KIND), obj)
    };
    let is_heap = b.ins().icmp_imm(IntCC::Equal, tag, HEAP_KIND);
    let chk = b.create_block();
    b.ins().brif(is_heap, chk, &[], slow, &[]);
    b.switch_to_block(chk);

    // 2. Heap index + generation select → the slot's machine address.
    let raw = b.ins().band_imm(raw_payload, 0xFFFF_FFFF);
    let rc = b.ins().load(types::I64, m, c.exec_ctx, heap_off as i32);
    let old_bit = b.ins().band_imm(raw, 0x8000_0000);
    let (base, idx) = if nursery_only {
        let cont = b.create_block();
        b.ins().brif(old_bit, slow, &[], cont, &[]);
        b.switch_to_block(cont);
        let base = b.ins().load(
            types::I64,
            m,
            rc,
            (alay.nursery_slots_vec_off + alay.slots_ptr_off) as i32,
        );
        (base, raw)
    } else {
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
        (base, idx)
    };
    let byte_off = b.ins().imul_imm(idx, alay.slot_size as i64);
    let slot_addr = b.ins().iadd(base, byte_off);

    // 3. Slot discriminant must be HeapObj::Instance or HeapObj::Object.
    let tagb = b.ins().uload8(types::I64, m, slot_addr, 0);
    let is_inst = b
        .ins()
        .icmp_imm(IntCC::Equal, tagb, olay.instance_tag as i64);
    let is_obj = b.ins().icmp_imm(IntCC::Equal, tagb, olay.object_tag as i64);
    let is_valid = b.ins().bor(is_inst, is_obj);
    let ok = b.create_block();
    b.ins().brif(is_valid, ok, &[], slow, &[]);
    b.switch_to_block(ok);

    // Instances have a COMPACT field layout (`class_field_repr` in
    // `varn-types::class_layout`): sizes 1/2/4/8, not a 16-byte `VmValue`
    // slot. The constant-offset addressing below assumes 16-byte slots, which
    // is only true for dynamic `Object`s, so send instances to the
    // compact-aware runtime helper (`get_fixed_field`/`set_fixed_field`).
    let obj_only = b.create_block();
    b.ins().brif(is_inst, slow, &[], obj_only, &[]);
    b.switch_to_block(obj_only);

    // 4. Load the payload pointer:
    // For Instance, payload is at instance_payload_off, and fields start at instance_values_off.
    // For Object, payload is at payload_off, and fields start at values_off.
    let c_inst_pay = b.ins().iconst(types::I64, olay.instance_payload_off as i64);
    let c_obj_pay = b.ins().iconst(types::I64, olay.payload_off as i64);
    let payload_off = b.ins().select(is_inst, c_inst_pay, c_obj_pay);
    let obj_ptr = b.ins().iadd(slot_addr, payload_off);
    let data_ptr = b.ins().load(types::I64, m, obj_ptr, 0);

    // 5. If it's an Object (not Instance), check slot < inline_len for overflow spill.
    // Instances have fixed layout and no overflow store.
    if slot >= 8 {
        let check_len_blk = b.create_block();
        let pass_blk = b.create_block();
        b.ins().brif(is_inst, pass_blk, &[], check_len_blk, &[]);

        b.switch_to_block(check_len_blk);
        let len = b.ins().load(types::I32, m, data_ptr, olay.len_off as i32);
        let in_bounds = b
            .ins()
            .icmp_imm(IntCC::UnsignedGreaterThan, len, slot as i64);
        b.ins().brif(in_bounds, pass_blk, &[], slow, &[]);

        b.switch_to_block(pass_blk);
    }

    // 6. Base address for inline values:
    let c_inst_val = b.ins().iconst(types::I64, olay.instance_values_off as i64);
    let c_obj_val = b.ins().iconst(types::I64, olay.values_off as i64);
    let values_off = b.ins().select(is_inst, c_inst_val, c_obj_val);
    let base_values = b.ins().iadd(data_ptr, values_off);
    b.ins().iadd_imm(base_values, (slot * 16) as i64)
}

/// `GetFixedField first_reg, obj, slot` — inline slot read, helper fallback;
/// result unboxed to int when the register meta proves it.
///
/// **Narrow-load optimisation** (static-typing exploitation):
/// When `register_meta[first_reg]` proves the destination is a primitive type
/// (`Int`, `Float`, `Bool`), every fast path loads ONLY the 8-byte payload
/// word at `slot*16 + 8`, skipping the tag word at `slot*16 + 0`. This
/// replaces a 16-byte i128 load + isplit + unbox with a single 8-byte i64
/// load — typically 3× fewer CLIF instructions on the hot path, which is the
/// inner loop of the DTO benchmark.
#[allow(clippy::too_many_arguments)]
fn emit_get_fixed_field_compact(
    b: &mut FunctionBuilder,
    c: &FldCtx,
    actx: Option<&AllocCtx>,
    state: &[K],
    first_reg: usize,
    obj_r: usize,
    offset: u32,
    tag: varn_core::TypeTag,
    slot: usize,
) -> Result<(), String> {
    use varn_types::class_layout::class_field_repr;

    let obj = if let Some(actx) = actx {
        super::alloc::box_or_load_home(b, actx, state, obj_r)
    } else {
        use_boxed(b, c.vars, state, obj_r)?
    };
    let (size, _align, is_gc_ref) = class_field_repr(tag);

    let slow = b.create_block();
    let cont = b.create_block();
    b.append_block_param(cont, types::I128);

    let data_base = emit::emit_object_data_base(
        b,
        c.exec_ctx,
        obj,
        &c.helpers.object_layout,
        &c.helpers.array_layout,
        c.helpers.heap_field_offset,
        slow,
    );
    let off = offset as i32;
    let m = MemFlags::trusted();
    let pair = match tag {
        varn_core::TypeTag::Bool => {
            let b8 = b.ins().load(types::I8, m, data_base, off);
            let v = b.ins().uextend(types::I64, b8);
            emit::box_bool(b, v)
        }
        varn_core::TypeTag::Int => {
            let v = b.ins().load(types::I64, m, data_base, off);
            emit::box_int(b, v)
        }
        varn_core::TypeTag::Float => {
            let f = b.ins().load(types::F64, m, data_base, off);
            emit::box_f64(b, f)
        }
        _ if is_gc_ref && size == 8 => {
            let raw = b.ins().load(types::I64, m, data_base, off);
            let is_uninit = b.ins().icmp_imm(
                IntCC::Equal,
                raw,
                u32::MAX as i64,
            );
            let null_tag = b
                .ins()
                .iconst(types::I64, varn_types::vm_value::KIND_NULL as i64);
            let heap_tag = b
                .ins()
                .iconst(types::I64, varn_types::vm_value::KIND_HEAP as i64);
            let zero = b.ins().iconst(types::I64, 0);
            let tag_v = b.ins().select(is_uninit, null_tag, heap_tag);
            let payload = b.ins().select(is_uninit, zero, raw);
            b.ins().iconcat(tag_v, payload)
        }
        _ => b.ins().load(types::I128, m, data_base, off),
    };
    b.ins().jump(cont, &[pair.into()]);

    // Slow path: not a compact instance (Object/Record/null). The dynamic
    // `slot` path handles those (shape-indexed).
    b.switch_to_block(slow);
    let (obj_tag, obj_payload) = b.ins().isplit(obj);
    let slot_v = b.ins().iconst(types::I64, slot as i64);
    call_helper_void(
        b,
        c.cc,
        c.helpers.get_fixed_field,
        &[c.exec_ctx, obj_tag, obj_payload, slot_v],
    );
    let res = b.ins().load(
        types::I128,
        MemFlags::trusted(),
        c.exec_ctx,
        c.helpers.jit_native_result_offset as i32,
    );
    b.ins().jump(cont, &[res.into()]);

    b.switch_to_block(cont);
    let res = b.block_params(cont)[0];
    if let Some(actx) = actx {
        super::alloc::def_result(b, actx, first_reg, res);
    } else {
        emit::def_boxed_leaf(b, c.register_meta, c.vars, first_reg, res);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn emit_get_fixed_field(
    b: &mut FunctionBuilder,
    c: &FldCtx,
    actx: Option<&AllocCtx>,
    state: &[K],
    code: &[u16],
    ip: usize,
    first_reg: usize,
) -> Result<(), String> {
    use varn_types::register_meta::SlotKind;

    let obj_r = (code[ip + 1] >> 8) as usize;
    let slot = code[ip + 2] as usize;
    // A non-`Null` field tag marks a compact CLASS field (baked offset in
    // `w3`); `Null` is a dynamic slot access (Object/Record/enum payload).
    let tag_byte = (code[ip + 1] & 0xFF) as u8;
    if tag_byte != 0 {
        return emit_get_fixed_field_compact(
            b,
            c,
            actx,
            state,
            first_reg,
            obj_r,
            code[ip + 3] as u32,
            varn_core::TypeTag::from_u8(tag_byte),
            slot,
        );
    }
    // Prefer the home slot (authoritative for a nullable `Ref` receiver, which
    // the variable cannot distinguish from a heap ref).
    let obj = if let Some(actx) = actx {
        super::alloc::box_or_load_home(b, actx, state, obj_r)
    } else {
        use_boxed(b, c.vars, state, obj_r)?
    };

    // ── Narrow-load decision ────────────────────────────────────────────
    // When the register meta statically proves the field is a primitive,
    // load only the i64 payload (offset +8 within each 16-byte VmValue
    // slot), avoiding the i128 load + isplit + unbox chain.
    let dest_kind = c.register_meta.get(first_reg).map(|m| m.kind);
    let narrow = matches!(
        dest_kind,
        Some(SlotKind::Int) | Some(SlotKind::Float) | Some(SlotKind::Bool)
    );

    let slow = b.create_block();
    let cont = b.create_block();

    let (cont_ty, load_ty) = if narrow {
        (types::I64, types::I64)
    } else {
        (types::I128, types::I128)
    };
    b.append_block_param(cont, cont_ty);

    let slot_off = (slot * 16) as i32;
    // For narrow loads: skip the 8-byte tag word, read the payload directly.
    let load_off = if narrow { slot_off + 8 } else { slot_off };

    // ── Fast path: loop-invariant cache ─────────────────────────────────
    if let Some(cache) = c.loop_caches.object(ip, obj_r) {
        let base = b.use_var(cache.data_base);
        let ok_cache = b.ins().icmp_imm(IntCC::NotEqual, base, 0);
        let fast_blk = b.create_block();
        let unhoisted_blk = b.create_block();
        b.ins().brif(ok_cache, fast_blk, &[], unhoisted_blk, &[]);

        b.switch_to_block(fast_blk);
        let val = b.ins().load(load_ty, MemFlags::trusted(), base, load_off);
        b.ins().jump(cont, &[val.into()]);

        b.switch_to_block(unhoisted_blk);
    }

    // ── Fast path: local object-base cache ──────────────────────────────
    if let Some(&local_var) = c.local_obj_bases.get(&obj_r) {
        let local_base = b.use_var(local_var);
        let ok_local = b.ins().icmp_imm(IntCC::NotEqual, local_base, 0);
        let fast_local = b.create_block();
        let miss_local = b.create_block();
        b.ins().brif(ok_local, fast_local, &[], miss_local, &[]);

        b.switch_to_block(fast_local);
        let val = b
            .ins()
            .load(load_ty, MemFlags::trusted(), local_base, load_off);
        b.ins().jump(cont, &[val.into()]);

        b.switch_to_block(miss_local);
        let computed_base = emit::emit_object_data_base(
            b,
            c.exec_ctx,
            obj,
            &c.helpers.object_layout,
            &c.helpers.array_layout,
            c.helpers.heap_field_offset,
            slow,
        );
        b.def_var(local_var, computed_base);
        let val = b
            .ins()
            .load(load_ty, MemFlags::trusted(), computed_base, load_off);
        b.ins().jump(cont, &[val.into()]);
    } else {
        // All fixed-field reads take the compact-aware runtime helper; the
        // inline 16-byte-slot address is wrong for compact instances and for
        // shapes whose slot indexing differs.
        let _ = (emit_object_field_addr, load_ty, load_off, slot_off);
        b.ins().jump(slow, &[]);
    }

    // ── Slow path: runtime helper ───────────────────────────────────────
    b.switch_to_block(slow);
    let slot_v = b.ins().iconst(types::I64, slot as i64);
    let (obj_tag, obj_payload) = if b.func.dfg.value_type(obj) == types::I128 {
        b.ins().isplit(obj)
    } else {
        (b.ins().iconst(types::I64, HEAP_KIND), obj)
    };
    call_helper_void(
        b,
        c.cc,
        c.helpers.get_fixed_field,
        &[c.exec_ctx, obj_tag, obj_payload, slot_v],
    );
    let res = b.ins().load(
        types::I128,
        MemFlags::trusted(),
        c.exec_ctx,
        c.helpers.jit_native_result_offset as i32,
    );
    if narrow {
        // Helper returns a full i128 VmValue; extract only the payload.
        let (_tag, payload) = b.ins().isplit(res);
        b.ins().jump(cont, &[payload.into()]);
    } else {
        b.ins().jump(cont, &[res.into()]);
    }

    // ── Result definition ───────────────────────────────────────────────
    b.switch_to_block(cont);
    let v = b.block_params(cont)[0];

    if narrow {
        // `v` is the raw i64 payload — no unboxing needed. The destination
        // CLASS decides storage: `Bool` is `SlotClass::Dyn`, so its variable is
        // an `I128` pair and must be boxed (writing the raw payload into it is
        // a Cranelift type error).
        match dest_kind {
            Some(SlotKind::Float) => {
                let f = b.ins().bitcast(types::F64, MemFlags::new(), v);
                b.def_var(c.vars[first_reg], f);
                if let Some(actx) = actx {
                    let payload = b.ins().bitcast(types::I64, MemFlags::new(), f);
                    let tag_v =
                        b.ins()
                            .iconst(types::I64, varn_types::vm_value::KIND_FLOAT as i64);
                    let boxed = b.ins().iconcat(tag_v, payload);
                    super::alloc::store_boxed_home(b, actx, first_reg, boxed);
                }
            }
            Some(SlotKind::Bool) => {
                emit::def_bool_result(b, actx, c.register_meta, c.vars, first_reg, v);
            }
            _ => {
                // Int: the payload IS the native value.
                emit::def_int_result(b, actx, c.register_meta, c.vars, first_reg, v);
            }
        }
    } else if let Some(actx) = actx {
        super::alloc::def_result(b, actx, first_reg, v);
    } else if meta_is_float(c.register_meta, first_reg) {
        let f = unbox_f64_coerce(b, v);
        b.def_var(c.vars[first_reg], f);
    } else if state_meta_int(c.register_meta, first_reg) {
        let i = unbox_int(b, v);
        b.def_var(c.vars[first_reg], i);
    } else {
        emit::def_boxed_leaf(b, c.register_meta, c.vars, first_reg, v);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn emit_set_fixed_field_compact(
    b: &mut FunctionBuilder,
    c: &FldCtx,
    actx: Option<&AllocCtx>,
    state: &[K],
    first_reg: usize,
    val_r: usize,
    offset: u32,
    tag: varn_core::TypeTag,
    slot: usize,
) -> Result<(), String> {
    use varn_types::class_layout::class_field_repr;

    let obj = if let Some(actx) = actx {
        super::alloc::box_or_load_home(b, actx, state, first_reg)
    } else {
        use_boxed(b, c.vars, state, first_reg)?
    };
    let val = if let Some(actx) = actx {
        super::alloc::box_or_load_home(b, actx, state, val_r)
    } else {
        box_or_pass(b, c.vars, state, val_r)
    };
    let val128 = if b.func.dfg.value_type(val) == types::I128 {
        val
    } else {
        emit::box_int(b, val)
    };
    let (size, _align, is_gc_ref) = class_field_repr(tag);

    let slow = b.create_block();
    let cont = b.create_block();
    let inline = b.create_block();

    // Write barrier: a store into an OLD-GEN instance needs the barrier the
    // runtime helper carries, so only a NURSERY receiver is inlined. Anything
    // not a heap ref, or old-gen, bails to `set_fixed_field_at`.
    {
        let (obj_tag, obj_payload) = b.ins().isplit(obj);
        let kind = b.ins().band_imm(obj_tag, emit::KIND_MASK);
        let heap_ok = b
            .ins()
            .icmp_imm(IntCC::Equal, kind, varn_types::vm_value::KIND_HEAP as i64);
        let raw = b.ins().band_imm(obj_payload, 0xFFFF_FFFF);
        let old_bit = b.ins().band_imm(raw, 0x8000_0000);
        let not_old = b.ins().icmp_imm(IntCC::Equal, old_bit, 0);
        let can_inline = b.ins().band(heap_ok, not_old);
        b.ins().brif(can_inline, inline, &[], slow, &[]);
    }

    b.switch_to_block(inline);
    let data_base = emit::emit_object_data_base(
        b,
        c.exec_ctx,
        obj,
        &c.helpers.object_layout,
        &c.helpers.array_layout,
        c.helpers.heap_field_offset,
        slow,
    );
    let off = offset as i32;
    let m = MemFlags::new();
    let (_vt, payload) = b.ins().isplit(val128);
    match tag {
        varn_core::TypeTag::Bool => {
            b.ins().istore8(m, payload, data_base, off);
        }
        varn_core::TypeTag::Int => {
            b.ins().store(m, payload, data_base, off);
        }
        varn_core::TypeTag::Float => {
            let f = unbox_f64_coerce(b, val128);
            b.ins().store(m, f, data_base, off);
        }
        _ if is_gc_ref && size == 8 => {
            let (tag_v, payload) = b.ins().isplit(val128);
            let is_null = b.ins().icmp_imm(
                IntCC::Equal,
                tag_v,
                varn_types::vm_value::KIND_NULL as i64,
            );
            let uninit = b.ins().iconst(types::I64, u32::MAX as i64);
            let stored = b.ins().select(is_null, uninit, payload);
            b.ins().store(m, stored, data_base, off);
        }
        _ => {
            b.ins().store(m, val128, data_base, off);
        }
    }
    b.ins().jump(cont, &[]);

    b.switch_to_block(slow);
    let (obj_tag, obj_payload) = b.ins().isplit(obj);
    let (v_tag, v_payload) = b.ins().isplit(val128);
    let slot_v = b.ins().iconst(types::I64, slot as i64);
    call_helper_void(
        b,
        c.cc,
        c.helpers.set_fixed_field,
        &[
            c.exec_ctx,
            obj_tag,
            obj_payload,
            slot_v,
            v_tag,
            v_payload,
        ],
    );
    b.ins().jump(cont, &[]);

    b.switch_to_block(cont);
    Ok(())
}

/// `SetFixedField obj(=first_reg), val, slot` — inline slot write for a nursery
/// receiver (no write barrier needed); an old-gen receiver bails to the helper,
/// which carries the barrier.
pub(super) fn emit_set_fixed_field(
    b: &mut FunctionBuilder,
    c: &FldCtx,
    actx: Option<&AllocCtx>,
    state: &[K],
    code: &[u16],
    ip: usize,
    first_reg: usize,
) -> Result<(), String> {
    let val_r = (code[ip + 1] >> 8) as usize;
    let slot = code[ip + 2] as usize;
    let tag_byte = (code[ip + 1] & 0xFF) as u8;
    if tag_byte != 0 {
        return emit_set_fixed_field_compact(
            b,
            c,
            actx,
            state,
            first_reg,
            val_r,
            code[ip + 3] as u32,
            varn_core::TypeTag::from_u8(tag_byte),
            slot,
        );
    }
    let obj = if let Some(actx) = actx {
        super::alloc::box_or_load_home(b, actx, state, first_reg)
    } else {
        use_boxed(b, c.vars, state, first_reg)?
    };
    let val = if let Some(actx) = actx {
        super::alloc::box_or_load_home(b, actx, state, val_r)
    } else {
        box_or_pass(b, c.vars, state, val_r)
    };
    let val128 = if b.func.dfg.value_type(val) == types::I128 {
        val
    } else if b.func.dfg.value_type(val) == types::F64 {
        super::emit::box_f64(b, val)
    } else {
        match state.get(val_r).copied().unwrap_or(K::Unset) {
            K::Int => super::emit::box_int(b, val),
            K::Bool => super::emit::box_bool(b, val),
            K::Float => {
                let f = b.ins().bitcast(types::F64, MemFlags::new(), val);
                super::emit::box_f64(b, f)
            }
            _ => {
                let tag_v = b.ins().iconst(types::I64, HEAP_KIND);
                b.ins().iconcat(tag_v, val)
            }
        }
    };

    let slow = b.create_block();
    let cont = b.create_block();

    // All fixed-field writes take the compact-aware runtime helper (write
    // barrier + compact layout); the inline 16-byte-slot address is wrong.
    let _ = (emit_object_field_addr, (slot * 16) as i32);
    b.ins().jump(slow, &[]);

    b.switch_to_block(slow);
    let slot_v = b.ins().iconst(types::I64, slot as i64);
    let (obj_tag, obj_payload) = if b.func.dfg.value_type(obj) == types::I128 {
        b.ins().isplit(obj)
    } else {
        (b.ins().iconst(types::I64, HEAP_KIND), obj)
    };
    let (val_tag, val_payload) = b.ins().isplit(val128);
    call_helper_void(
        b,
        c.cc,
        c.helpers.set_fixed_field,
        &[
            c.exec_ctx,
            obj_tag,
            obj_payload,
            slot_v,
            val_tag,
            val_payload,
        ],
    );
    b.ins().jump(cont, &[]);

    b.switch_to_block(cont);
    Ok(())
}
