//! Property, index and field access for the SSA lowering.
//!
//! All of these go through boxed runtime helpers that may run user code (a
//! getter/setter) or inspect the heap, so they only appear in a frame-aware
//! body. Operands are read from their homes, boxed as needed, and a result is
//! landed in the destination's home. `GetProperty`/`SetProperty` use the flat
//! helpers with the inline-cache slot the projection numbered; `ip` is 0
//! because the lowering refuses `Try`, so this frame is never resumed
//! interpreted at a bytecode offset.

use cranelift_codegen::ir::{condcodes::IntCC, types, InstBuilder, MemFlags, Value};
use cranelift_frontend::FunctionBuilder;

use super::heap::{boxed_parts, exec_ctx};
use super::{use_heap, Ctx};

use super::super::emit::call_helper_void;

/// `arr.length` — a boxed `int` result.
pub(super) fn emit_array_length(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    operand: u32,
) -> Result<Value, String> {
    let (tag, payload) = boxed_parts(b, ctx, values, operand)?;
    let ectx = exec_ctx(ctx);
    call_helper_void(b, ctx.cc, ctx.helpers.array_length, &[ectx, tag, payload]);
    Ok(b.ins().load(
        types::I128,
        MemFlags::trusted(),
        ectx,
        ctx.helpers.jit_native_result_offset as i32,
    ))
}

/// `s.length` of a `str` — the boxed `int` length.
pub(super) fn emit_str_length(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    operand: u32,
) -> Result<Value, String> {
    let (tag, payload) = boxed_parts(b, ctx, values, operand)?;
    let ectx = exec_ctx(ctx);
    Ok(super::super::strings::str_length_boxed(
        b,
        ctx.cc,
        ctx.helpers.str_length,
        ectx,
        ctx.helpers.jit_native_result_offset as i32,
        tag,
        payload,
    ))
}

/// `b.length` of `Bytes` — a boxed `int` result.
pub(super) fn emit_bytes_length(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    operand: u32,
) -> Result<Value, String> {
    let (tag, payload) = boxed_parts(b, ctx, values, operand)?;
    let ectx = exec_ctx(ctx);
    call_helper_void(b, ctx.cc, ctx.helpers.bytes_length, &[ectx, tag, payload]);
    Ok(b.ins().load(
        types::I128,
        MemFlags::trusted(),
        ectx,
        ctx.helpers.jit_native_result_offset as i32,
    ))
}

/// `arr.push(value)` — no result.
pub(super) fn emit_array_push(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    array: u32,
    value: u32,
) -> Result<(), String> {
    let (at, ap) = boxed_parts(b, ctx, values, array)?;
    let (vt, vp) = boxed_parts(b, ctx, values, value)?;
    let ectx = exec_ctx(ctx);
    call_helper_void(b, ctx.cc, ctx.helpers.array_push, &[ectx, at, ap, vt, vp]);
    Ok(())
}

/// `obj[index]` — a heap result.
pub(super) fn emit_get_index(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    object: u32,
    index: u32,
) -> Result<Value, String> {
    let (ot, op) = boxed_parts(b, ctx, values, object)?;
    let (kt, kp) = boxed_parts(b, ctx, values, index)?;
    let ectx = exec_ctx(ctx);
    call_helper_void(
        b,
        ctx.cc,
        ctx.helpers.jit_array_get_fast,
        &[ectx, ot, op, kt, kp],
    );
    Ok(b.ins().load(
        types::I128,
        MemFlags::trusted(),
        ectx,
        ctx.helpers.jit_native_result_offset as i32,
    ))
}

/// `obj[index] = value` — no result.
pub(super) fn emit_set_index(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    object: u32,
    index: u32,
    value: u32,
) -> Result<(), String> {
    let (ot, op) = boxed_parts(b, ctx, values, object)?;
    let (kt, kp) = boxed_parts(b, ctx, values, index)?;
    let (vt, vp) = boxed_parts(b, ctx, values, value)?;
    let ectx = exec_ctx(ctx);
    call_helper_void(
        b,
        ctx.cc,
        ctx.helpers.jit_array_set_fast,
        &[ectx, ot, op, kt, kp, vt, vp],
    );
    Ok(())
}

/// `this` — the receiver, from home 0.
pub(super) fn emit_this(b: &mut FunctionBuilder, ctx: &Ctx<'_>) -> Result<Value, String> {
    use_heap(b, ctx, 0)
}

/// What a compact field access needs from this body.
fn field_io<'a>(ctx: &'a Ctx<'_>) -> Result<super::super::fields::FieldIo<'a>, String> {
    Ok(super::super::fields::FieldIo {
        helpers: ctx.helpers,
        cc: ctx.cc,
        exec_ctx: exec_ctx(ctx),
    })
}

/// `obj.field` — a class field at its compact offset, inline, or an
/// object/record/enum-payload field by slot through the helper. A scalar
/// destination with a matching scalar field class comes back native (no
/// box/unbox round trip); everything else comes back boxed.
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_get_fixed_field(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    object: u32,
    slot: u16,
    offset: u32,
    access: varn_core::FieldAccess,
    dest: Option<u32>,
) -> Result<super::store::Out, String> {
    use varn_types::layout::{ScalarRepr, TypeLayout};
    use varn_types::register_meta::SlotKind;
    let obj = super::heap::boxed_value(b, ctx, values, object)?;
    if let (Some(d), varn_core::FieldAccess::Compact(tag)) = (dest, access) {
        let native = matches!(
            (ctx.ssa.value_ty(d), TypeLayout::of_field(tag).repr),
            (SlotKind::Int, ScalarRepr::I64)
                | (SlotKind::Float, ScalarRepr::F64)
                | (SlotKind::Bool, ScalarRepr::Bool)
        );
        if native {
            let v =
                emit_get_fixed_field_native(b, ctx, obj, offset, tag, slot, ctx.ssa.value_ty(d))?;
            return Ok(super::store::Out::Native(v));
        }
    }
    if let varn_core::FieldAccess::Compact(kind) = access {
        return Ok(super::store::Out::Boxed(
            super::super::fields::load_compact(
                b,
                &field_io(ctx)?,
                obj,
                offset,
                kind,
                slot as usize,
            ),
        ));
    }
    let (ot, op) = b.ins().isplit(obj);
    let ectx = exec_ctx(ctx);
    let slot_v = b.ins().iconst(types::I64, slot as i64);
    call_helper_void(
        b,
        ctx.cc,
        ctx.helpers.get_fixed_field,
        &[ectx, ot, op, slot_v],
    );
    Ok(super::store::Out::Boxed(b.ins().load(
        types::I128,
        MemFlags::trusted(),
        ectx,
        ctx.helpers.jit_native_result_offset as i32,
    )))
}

/// Native-classed read of a compact field: the same inline guard as
/// [`load_compact`][super::super::fields::load_compact], but the fast path
/// lands the raw payload and the slow path unboxes the helper's boxed
/// result — never a box/unbox round trip.
fn emit_get_fixed_field_native(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    obj: Value,
    offset: u32,
    tag: Option<varn_core::RuntimeKind>,
    slot: u16,
    dest: varn_types::register_meta::SlotKind,
) -> Result<Value, String> {
    use varn_types::layout::{ScalarRepr, TypeLayout};
    let h = ctx.helpers;
    let ectx = exec_ctx(ctx);
    let merge_ty = match dest {
        varn_types::register_meta::SlotKind::Float => types::F64,
        _ => types::I64,
    };
    let slow = b.create_block();
    let cont = b.create_block();
    b.append_block_param(cont, merge_ty);
    let data_base = super::super::emit::emit_object_data_base(
        b,
        ectx,
        obj,
        &h.object_layout,
        &h.array_layout,
        h.heap_field_offset,
        slow,
    );
    let off = offset as i32;
    let m = MemFlags::trusted();
    let v = match TypeLayout::of_field(tag).repr {
        ScalarRepr::I64 => b.ins().load(types::I64, m, data_base, off),
        ScalarRepr::F64 => b.ins().load(types::F64, m, data_base, off),
        ScalarRepr::Bool => {
            let b8 = b.ins().load(types::I8, m, data_base, off);
            b.ins().uextend(types::I64, b8)
        }
        _ => {
            return Err("from_ssa: native field read of a non-scalar repr".into());
        }
    };
    b.ins().jump(cont, &[v.into()]);
    b.switch_to_block(slow);
    let (ot, op) = b.ins().isplit(obj);
    let slot_v = b.ins().iconst(types::I64, slot as i64);
    call_helper_void(b, ctx.cc, h.get_fixed_field, &[ectx, ot, op, slot_v]);
    let boxed = b.ins().load(
        types::I128,
        MemFlags::trusted(),
        ectx,
        h.jit_native_result_offset as i32,
    );
    let back = super::heap::unbox_dest(b, dest, boxed)?;
    b.ins().jump(cont, &[back.into()]);
    b.switch_to_block(cont);
    Ok(b.block_params(cont)[0])
}

/// `obj.field = value` — a class field at its compact offset, inline for a
/// nursery receiver (the same lowering as the bytecode path).
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_set_fixed_field(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    object: u32,
    value: u32,
    slot: u16,
    offset: u32,
    kind: Option<varn_core::RuntimeKind>,
) -> Result<(), String> {
    let obj = super::heap::boxed_value(b, ctx, values, object)?;
    let val = super::heap::boxed_value(b, ctx, values, value)?;
    super::super::fields::store_compact(b, &field_io(ctx)?, obj, val, offset, kind, slot as usize);
    Ok(())
}

/// `obj.name` — dynamic property read. Monomorphic/polymorphic shape fast
/// path inline, generic helper as fallback.
///
/// Fast path: when the receiver is a heap `Object` whose shape id matches a
/// `SHAPE_PROP` entry of this site's inline cache, the field is one `I128`
/// load from the object's inline tail — no string lookup, no heap extract,
/// no FFI. Anything else (non-object receiver, shape miss, overflowed slot,
/// getter/method) takes the `get_property_flat` helper, which owns the full
/// semantics and populates the cache for next time.
///
/// GC safety: the fast path performs only reads (no allocation, no call), so
/// no collection can run under it; the value is written to the destination
/// home before rejoining, exactly as the helper does.
pub(super) fn emit_get_property(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    object: u32,
    name: &str,
    cs: u16,
    dest_reg: u32,
) -> Result<Value, String> {
    let (ot, op) = boxed_parts(b, ctx, values, object)?;
    let frame = ctx
        .frame
        .as_ref()
        .ok_or("from_ssa: property access without a frame")?;
    let name_idx = str_idx(ctx, name)?;
    let ectx = frame.exec_ctx;

    let slow = b.create_block();
    b.set_cold_block(slow);
    let merge = b.create_block();
    b.append_block_param(merge, types::I128);

    let m = MemFlags::trusted();
    let olay = &ctx.helpers.object_layout;
    let alay = &ctx.helpers.array_layout;
    let heap_off = ctx.helpers.heap_field_offset;

    // 1. Heap-tag check; non-heap receivers take the helper.
    let kind = b.ins().band_imm(ot, super::super::emit::KIND_MASK);
    let is_heap = b
        .ins()
        .icmp_imm(IntCC::Equal, kind, super::super::emit::HEAP_KIND);
    let chk = b.create_block();
    b.ins().brif(is_heap, chk, &[], slow, &[]);
    b.switch_to_block(chk);

    // 2. Heap index + generation select → slot address.
    let raw = b.ins().band_imm(op, 0xFFFF_FFFF);
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

    // 3. Slot must be a heap `Object` (records/instances/methods → helper).
    let tagb = b.ins().uload8(types::I64, m, slot_addr, 0);
    let is_obj = b.ins().icmp_imm(IntCC::Equal, tagb, olay.object_tag as i64);
    let ok = b.create_block();
    b.ins().brif(is_obj, ok, &[], slow, &[]);
    b.switch_to_block(ok);

    // 4. Payload → data pointer → shape id, inline length, values base.
    let data_ptr = b
        .ins()
        .load(types::I64, m, slot_addr, olay.payload_off as i32);
    let shape_ctl = b.ins().load(types::I64, m, data_ptr, olay.shape_off as i32);
    let shape_id32 = b
        .ins()
        .load(types::I32, m, shape_ctl, olay.shape_id_off as i32);
    let shape_id = b.ins().uextend(types::I64, shape_id32);
    let len32 = b.ins().load(types::I32, m, data_ptr, olay.len_off as i32);
    let inline_len = b.ins().uextend(types::I64, len32);
    let values_base = b.ins().iadd_imm(data_ptr, olay.values_off as i64);

    // 5. Probe the site's 8 cache entries for a SHAPE_PROP hit on this shape.
    // Each entry is 8 bytes: id u32 @0, slot u16 @4, is_class u8 @6.
    let ic_base = b.ins().load(
        types::I64,
        m,
        frame.closure,
        ctx.helpers.closure_ic_entries_offset as i32,
    );
    let slot_base = b.ins().iadd_imm(
        ic_base,
        (cs as i64) * (ctx.helpers.poly_ic_slot_size as i64),
    );
    let mut next = b.create_block();
    // First check branches out of the resolve block; the rest chain.
    b.ins().jump(next, &[]);
    for i in 0..8 {
        b.switch_to_block(next);
        next = b.create_block();
        let hit = b.create_block();
        let entry = b.ins().iadd_imm(slot_base, (i * 8) as i64);
        let id32 = b.ins().load(types::I32, m, entry, 0);
        let id = b.ins().uextend(types::I64, id32);
        let kc = b.ins().uload8(types::I64, m, entry, 6);
        let id_eq = b.ins().icmp(IntCC::Equal, id, shape_id);
        let kind_ok = b.ins().icmp_imm(
            IntCC::Equal,
            kc,
            varn_types::chunk::ICKind::SHAPE_PROP as i64,
        );
        let matched = b.ins().band(id_eq, kind_ok);
        b.ins().brif(matched, hit, &[], next, &[]);

        b.switch_to_block(hit);
        let slot16 = b.ins().uload16(types::I64, m, entry, 4);
        let in_bounds = b.ins().icmp(IntCC::UnsignedLessThan, slot16, inline_len);
        let hok = b.create_block();
        b.ins().brif(in_bounds, hok, &[], slow, &[]);
        b.switch_to_block(hok);
        let off = b.ins().ishl_imm(slot16, 4);
        let addr = b.ins().iadd(values_base, off);
        let val = b.ins().load(types::I128, m, addr, 0);
        super::store::def_heap(b, ctx, dest_reg, val)?;
        super::store::drop_home_addrs(ctx);
        b.ins().jump(merge, &[val.into()]);
    }
    // No entry matched → generic helper.
    b.switch_to_block(next);
    b.ins().jump(slow, &[]);

    b.switch_to_block(slow);
    let name_v = b.ins().iconst(types::I64, name_idx as i64);
    let cs_v = b.ins().iconst(types::I64, cs as i64);
    let dest_v = b.ins().iconst(types::I64, dest_reg as i64);
    let ip_v = b.ins().iconst(types::I64, 0);
    call_helper_void(
        b,
        ctx.cc,
        ctx.helpers.get_property_flat,
        &[
            ectx,
            frame.closure,
            frame.base,
            ot,
            op,
            name_v,
            cs_v,
            dest_v,
            ip_v,
        ],
    );
    // The helper above runs user getters, which may push frames and
    // reallocate the home vectors: drop any memoized address before reading
    // the value it just wrote.
    super::store::drop_home_addrs(ctx);
    let slow_val = use_heap(b, ctx, dest_reg)?;
    super::store::drop_home_addrs(ctx);
    b.ins().jump(merge, &[slow_val.into()]);

    b.switch_to_block(merge);
    Ok(b.block_params(merge)[0])
}

/// `obj.name = value` — dynamic property write. Nursery-only shape fast
/// path inline, generic helper as fallback.
///
/// Fast path: heap `Object` in the nursery whose shape id matches a
/// `SHAPE_PROP` entry of this site's cache gets one `I128` store into the
/// inline tail. Nursery-only keeps the store barrier-free, the same rule
/// `store_compact` uses: an old-generation receiver (or a miss, an
/// overflowed slot, a transition, a setter) takes `set_property_flat`, which
/// owns the full semantics and populates the cache.
pub(super) fn emit_set_property(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    object: u32,
    value: u32,
    name: &str,
    cs: u16,
) -> Result<(), String> {
    let (ot, op) = boxed_parts(b, ctx, values, object)?;
    let val = super::heap::boxed_value(b, ctx, values, value)?;
    let frame = ctx
        .frame
        .as_ref()
        .ok_or("from_ssa: property write without a frame")?;
    let name_idx = str_idx(ctx, name)?;
    let ectx = frame.exec_ctx;

    let slow = b.create_block();
    b.set_cold_block(slow);
    let cont = b.create_block();

    let m = MemFlags::trusted();
    let olay = &ctx.helpers.object_layout;
    let alay = &ctx.helpers.array_layout;
    let heap_off = ctx.helpers.heap_field_offset;

    // 1. Heap-tag check.
    let kind = b.ins().band_imm(ot, super::super::emit::KIND_MASK);
    let is_heap = b
        .ins()
        .icmp_imm(IntCC::Equal, kind, super::super::emit::HEAP_KIND);
    let chk = b.create_block();
    b.ins().brif(is_heap, chk, &[], slow, &[]);
    b.switch_to_block(chk);

    // 2. Nursery-only: old-generation stores need the write barrier the
    // helper carries.
    let raw = b.ins().band_imm(op, 0xFFFF_FFFF);
    let old_bit = b.ins().band_imm(raw, 0x8000_0000);
    let is_nursery = b.ins().icmp_imm(IntCC::Equal, old_bit, 0);
    let res = b.create_block();
    b.ins().brif(is_nursery, res, &[], slow, &[]);
    b.switch_to_block(res);

    // 3. Slot address (nursery base) + Object tag check.
    let rc = b.ins().load(types::I64, m, ectx, heap_off as i32);
    let base_nur = b.ins().load(
        types::I64,
        m,
        rc,
        (alay.nursery_slots_vec_off + alay.slots_ptr_off) as i32,
    );
    let byte_off = b.ins().imul_imm(raw, alay.slot_size as i64);
    let slot_addr = b.ins().iadd(base_nur, byte_off);
    let tagb = b.ins().uload8(types::I64, m, slot_addr, 0);
    let is_obj = b.ins().icmp_imm(IntCC::Equal, tagb, olay.object_tag as i64);
    let ok = b.create_block();
    b.ins().brif(is_obj, ok, &[], slow, &[]);
    b.switch_to_block(ok);

    // 4. Shape id, inline length, values base.
    let data_ptr = b
        .ins()
        .load(types::I64, m, slot_addr, olay.payload_off as i32);
    let shape_ctl = b.ins().load(types::I64, m, data_ptr, olay.shape_off as i32);
    let shape_id32 = b
        .ins()
        .load(types::I32, m, shape_ctl, olay.shape_id_off as i32);
    let shape_id = b.ins().uextend(types::I64, shape_id32);
    let len32 = b.ins().load(types::I32, m, data_ptr, olay.len_off as i32);
    let inline_len = b.ins().uextend(types::I64, len32);
    let values_base = b.ins().iadd_imm(data_ptr, olay.values_off as i64);

    // 5. Probe the 8 entries for a SHAPE_PROP hit; each hit stores inline.
    let ic_base = b.ins().load(
        types::I64,
        m,
        frame.closure,
        ctx.helpers.closure_ic_entries_offset as i32,
    );
    let slot_base = b.ins().iadd_imm(
        ic_base,
        (cs as i64) * (ctx.helpers.poly_ic_slot_size as i64),
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
        let id_eq = b.ins().icmp(IntCC::Equal, id, shape_id);
        let kind_ok = b.ins().icmp_imm(
            IntCC::Equal,
            kc,
            varn_types::chunk::ICKind::SHAPE_PROP as i64,
        );
        let matched = b.ins().band(id_eq, kind_ok);
        b.ins().brif(matched, hit, &[], next, &[]);

        b.switch_to_block(hit);
        let slot16 = b.ins().uload16(types::I64, m, entry, 4);
        let in_bounds = b.ins().icmp(IntCC::UnsignedLessThan, slot16, inline_len);
        let hok = b.create_block();
        b.ins().brif(in_bounds, hok, &[], slow, &[]);
        b.switch_to_block(hok);
        let off = b.ins().ishl_imm(slot16, 4);
        let addr = b.ins().iadd(values_base, off);
        b.ins().store(MemFlags::trusted(), val, addr, 0);
        b.ins().jump(cont, &[]);
    }
    b.switch_to_block(next);
    b.ins().jump(slow, &[]);

    b.switch_to_block(slow);
    let (vt, vp) = b.ins().isplit(val);
    let name_v = b.ins().iconst(types::I64, name_idx as i64);
    let cs_v = b.ins().iconst(types::I64, cs as i64);
    let ip_v = b.ins().iconst(types::I64, 0);
    call_helper_void(
        b,
        ctx.cc,
        ctx.helpers.set_property_flat,
        &[ectx, frame.closure, ot, op, vt, vp, name_v, cs_v, ip_v],
    );
    // May run a setter / push frames: any memoized home address is stale.
    // This op returns nothing, so clearing is enough (no reload).
    super::store::drop_home_addrs(ctx);
    b.ins().jump(cont, &[]);

    b.switch_to_block(cont);
    Ok(())
}

/// Pool index of a string constant, 1:1 with the resolved constants.
pub(super) fn str_idx(ctx: &Ctx<'_>, s: &str) -> Result<usize, String> {
    ctx.proto
        .chunk
        .constants
        .iter()
        .position(|e| {
            matches!(e, varn_types::PoolEntry::Literal(varn_types::Literal::Str(t)) if t.as_ref() == s)
        })
        .ok_or_else(|| format!("from_ssa: string {s:?} not in pool"))
}
