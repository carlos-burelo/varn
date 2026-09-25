//! Property, index and field access for the SSA lowering.
//!
//! All of these go through boxed runtime helpers that may run user code (a
//! getter/setter) or inspect the heap, so they only appear in a frame-aware
//! body. Operands are read from their homes, boxed as needed, and a result is
//! landed in the destination's home. `GetProperty`/`SetProperty` use the flat
//! helpers with the inline-cache slot the projection numbered; `ip` is 0
//! because the lowering refuses `Try`, so this frame is never resumed
//! interpreted at a bytecode offset.

use cranelift_codegen::ir::{types, InstBuilder, MemFlags, Value};
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
    let ectx = exec_ctx(ctx)?;
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
    let ectx = exec_ctx(ctx)?;
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
    let ectx = exec_ctx(ctx)?;
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
    let ectx = exec_ctx(ctx)?;
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
    let ectx = exec_ctx(ctx)?;
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
        exec_ctx: exec_ctx(ctx)?,
    })
}

/// `obj.field` — a class field at its compact offset, inline (the same
/// lowering as the bytecode path), or an object/record/enum-payload field by
/// slot through the helper. The boxed value.
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_get_fixed_field(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    object: u32,
    slot: u16,
    offset: u32,
    access: varn_core::FieldAccess,
) -> Result<Value, String> {
    let obj = super::heap::boxed_value(b, ctx, values, object)?;
    if let varn_core::FieldAccess::Compact(kind) = access {
        return Ok(super::super::fields::load_compact(
            b,
            &field_io(ctx)?,
            obj,
            offset,
            kind,
            slot as usize,
        ));
    }
    let (ot, op) = b.ins().isplit(obj);
    let ectx = exec_ctx(ctx)?;
    let slot_v = b.ins().iconst(types::I64, slot as i64);
    call_helper_void(
        b,
        ctx.cc,
        ctx.helpers.get_fixed_field,
        &[ectx, ot, op, slot_v],
    );
    Ok(b.ins().load(
        types::I128,
        MemFlags::trusted(),
        ectx,
        ctx.helpers.jit_native_result_offset as i32,
    ))
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

/// `obj.name` — dynamic property read. The helper writes the boxed result into
/// the destination's home (`dest_reg`), so the value is rooted before any
/// getter runs; the caller then unboxes it for a scalar dest.
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
    use_heap(b, ctx, dest_reg)
}

/// `obj.name = value` — dynamic property write (may run a setter, hence GC).
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
    let (vt, vp) = boxed_parts(b, ctx, values, value)?;
    let frame = ctx
        .frame
        .as_ref()
        .ok_or("from_ssa: property write without a frame")?;
    let name_idx = str_idx(ctx, name)?;
    let ectx = frame.exec_ctx;
    let name_v = b.ins().iconst(types::I64, name_idx as i64);
    let cs_v = b.ins().iconst(types::I64, cs as i64);
    let ip_v = b.ins().iconst(types::I64, 0);
    call_helper_void(
        b,
        ctx.cc,
        ctx.helpers.set_property_flat,
        &[ectx, frame.closure, ot, op, vt, vp, name_v, cs_v, ip_v],
    );
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
