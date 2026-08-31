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

use super::emit::{
    self, box_or_pass, call_helper, call_helper_void, meta_is_float, state_meta_int,
    unbox_f64_coerce, use_boxed, wrap_i48, HEAP_KIND, KIND_MASK,
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
    let tag = b.ins().band_imm(obj, KIND_MASK);
    let is_heap = b.ins().icmp_imm(IntCC::Equal, tag, HEAP_KIND);
    let chk = b.create_block();
    b.ins().brif(is_heap, chk, &[], slow, &[]);
    b.switch_to_block(chk);

    // 2. Heap index + generation select → the slot's machine address.
    let raw = b.ins().band_imm(obj, 0xFFFF_FFFF);
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

    // 3. Slot discriminant must be HeapObj::Object.
    let tagb = b.ins().uload8(types::I64, m, slot_addr, 0);
    let is_obj = b.ins().icmp_imm(IntCC::Equal, tagb, olay.object_tag as i64);
    let ok = b.create_block();
    b.ins().brif(is_obj, ok, &[], slow, &[]);
    b.switch_to_block(ok);

    // 4. Load the object's ObjData pointer.
    let objdata = b
        .ins()
        .load(types::I64, m, slot_addr, olay.payload_off as i32);

    // 5. slot < inline_len — fields past the inline tail spilled to the
    //    overflow store, which only the helper can read/write.
    let len = b.ins().load(types::I32, m, objdata, olay.len_off as i32);
    let in_bounds = b
        .ins()
        .icmp_imm(IntCC::UnsignedGreaterThan, len, slot as i64);
    let ok2 = b.create_block();
    b.ins().brif(in_bounds, ok2, &[], slow, &[]);
    b.switch_to_block(ok2);

    // 6. The field is inline in the same allocation: one constant displacement.
    b.ins()
        .iadd_imm(objdata, (olay.values_off + slot * 8) as i64)
}

/// `GetFixedField first_reg, obj, slot` — inline slot read, helper fallback;
/// result unboxed to int when the register meta proves it.
pub(super) fn emit_get_fixed_field(
    b: &mut FunctionBuilder,
    c: &FldCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
    first_reg: usize,
) -> Result<(), String> {
    let obj_r = (code[ip + 1] >> 8) as usize;
    let slot = code[ip + 2] as usize;
    let obj = use_boxed(b, c.vars, state, obj_r)?;

    let slow = b.create_block();
    let cont = b.create_block();
    b.append_block_param(cont, types::I64);

    let slot_off = (slot * 8) as i32;
    if let Some(cache) = c.loop_caches.object(ip, obj_r) {
        let base = b.use_var(cache.data_base);
        let ok_cache = b.ins().icmp_imm(IntCC::NotEqual, base, 0);
        let fast_blk = b.create_block();
        let unhoisted_blk = b.create_block();
        b.ins().brif(ok_cache, fast_blk, &[], unhoisted_blk, &[]);

        b.switch_to_block(fast_blk);
        let val = b
            .ins()
            .load(types::I64, MemFlags::trusted(), base, slot_off);
        b.ins().jump(cont, &[val.into()]);

        b.switch_to_block(unhoisted_blk);
    }

    let field = emit_object_field_addr(b, c, obj, slot, slow, false);
    let val = b.ins().load(types::I64, MemFlags::trusted(), field, 0);
    b.ins().jump(cont, &[val.into()]);

    b.switch_to_block(slow);
    let slot_v = b.ins().iconst(types::I64, slot as i64);
    let res = call_helper(
        b,
        c.cc,
        c.helpers.get_fixed_field,
        &[c.exec_ctx, obj, slot_v],
    );
    b.ins().jump(cont, &[res.into()]);

    b.switch_to_block(cont);
    let v = b.block_params(cont)[0];
    if meta_is_float(c.register_meta, first_reg) {
        let f = unbox_f64_coerce(b, v);
        b.def_var(c.vars[first_reg], f);
    } else if state_meta_int(c.register_meta, first_reg) {
        let i = wrap_i48(b, v);
        b.def_var(c.vars[first_reg], i);
    } else {
        b.def_var(c.vars[first_reg], v);
    }
    Ok(())
}

/// `SetFixedField obj(=first_reg), val, slot` — inline slot write for a nursery
/// receiver (no write barrier needed); an old-gen receiver bails to the helper,
/// which carries the barrier.
pub(super) fn emit_set_fixed_field(
    b: &mut FunctionBuilder,
    c: &FldCtx,
    state: &[K],
    code: &[u16],
    ip: usize,
    first_reg: usize,
) -> Result<(), String> {
    let val_r = (code[ip + 1] >> 8) as usize;
    let slot = code[ip + 2] as usize;
    let obj = use_boxed(b, c.vars, state, first_reg)?;
    let val = box_or_pass(b, c.vars, state, val_r);

    let slow = b.create_block();
    let cont = b.create_block();

    let slot_off = (slot * 8) as i32;
    if let Some(cache) = c.loop_caches.object(ip, first_reg) {
        let base = b.use_var(cache.data_base);
        let ok_cache = b.ins().icmp_imm(IntCC::NotEqual, base, 0);
        let fast_blk = b.create_block();
        let unhoisted_blk = b.create_block();
        b.ins().brif(ok_cache, fast_blk, &[], unhoisted_blk, &[]);

        b.switch_to_block(fast_blk);
        b.ins().store(MemFlags::trusted(), val, base, slot_off);
        b.ins().jump(cont, &[]);

        b.switch_to_block(unhoisted_blk);
    }

    let field = emit_object_field_addr(b, c, obj, slot, slow, true);
    b.ins().store(MemFlags::trusted(), val, field, 0);
    b.ins().jump(cont, &[]);

    b.switch_to_block(slow);
    let slot_v = b.ins().iconst(types::I64, slot as i64);
    call_helper_void(
        b,
        c.cc,
        c.helpers.set_fixed_field,
        &[c.exec_ctx, obj, slot_v, val],
    );
    b.ins().jump(cont, &[]);

    b.switch_to_block(cont);
    Ok(())
}
