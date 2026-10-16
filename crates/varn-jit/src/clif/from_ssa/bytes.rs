use cranelift_codegen::ir::{condcodes::IntCC, types, Block, InstBuilder, Value};
use cranelift_frontend::FunctionBuilder;
use varn_types::register_meta::SlotKind;

use super::super::emit::{box_int, unbox_int, HEAP_KIND, KIND_MASK};
use super::{heap, load_value, Ctx, Out};
use crate::JitBufferLayout;

const BYTES_VIEW_DISC: i64 = 3;

pub(super) fn emit_get(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    object: u32,
    index: u32,
    dest: Option<u32>,
) -> Result<Out, String> {
    let lay = ctx.helpers.buffer_layout;
    let kind_off = ctx.helpers.array_layout.kind_off as i32;
    let exec_ctx = heap::exec_ctx(ctx);
    let key = load_value(b, ctx, values, index)?;
    let obj = heap::boxed_value(b, ctx, values, object)?;

    let slow = b.create_block();
    b.set_cold_block(slow);
    let merge = b.create_block();
    b.append_block_param(merge, types::I64);

    let (data, len) = match ctx.views.of(object) {
        Some(v) => {
            let data = b.use_var(v.data);
            let cached_len = b.use_var(v.len);
            let cached_disc = b.use_var(v.disc);
            let ready = b.create_block();
            b.append_block_param(ready, types::I64);
            b.append_block_param(ready, types::I64);
            let miss = b.create_block();
            b.ins()
                .brif(data, ready, &[data.into(), cached_len.into()], miss, &[]);
            let _ = cached_disc;
            b.switch_to_block(miss);
            let (obj_tag, obj_payload) = b.ins().isplit(obj);
            let (d, l) = resolve_parts(b, &lay, kind_off, obj_tag, obj_payload, slow);
            b.def_var(v.data, d);
            b.def_var(v.len, l);
            let disc = b.ins().iconst(types::I64, BYTES_VIEW_DISC);
            b.def_var(v.disc, disc);
            super::store::drop_home_addrs(ctx);
            b.ins().jump(ready, &[d.into(), l.into()]);
            b.switch_to_block(ready);
            let p = b.block_params(ready);
            (p[0], p[1])
        }
        None => {
            let (obj_tag, obj_payload) = b.ins().isplit(obj);
            resolve_parts(b, &lay, kind_off, obj_tag, obj_payload, slow)
        }
    };
    let m = cranelift_codegen::ir::MemFlagsData::trusted();
    let in_bounds = b.ins().icmp(IntCC::UnsignedLessThan, key, len);
    let hit = b.create_block();
    b.ins().brif(in_bounds, hit, &[], slow, &[]);
    b.switch_to_block(hit);
    let at = b.ins().iadd(data, key);
    let v = b.ins().uload8(types::I64, m, at, 0);
    b.ins().jump(merge, &[v.into()]);

    b.switch_to_block(slow);
    let (ot, op) = b.ins().isplit(obj);
    let boxed_key = box_int(b, key);
    let (kt, kp) = b.ins().isplit(boxed_key);
    crate::clif::from_ssa::exceptions::call_checked(
        b,
        ctx,
        ctx.helpers.jit_array_get_fast,
        &[exec_ctx, ot, op, kt, kp],
    )?;
    let r = b.ins().load(
        types::I128,
        cranelift_codegen::ir::MemFlagsData::trusted(),
        exec_ctx,
        ctx.helpers.jit_native_result_offset as i32,
    );
    let r = unbox_int(b, r);
    super::store::drop_home_addrs(ctx);
    b.ins().jump(merge, &[r.into()]);

    b.switch_to_block(merge);
    let res = b.block_params(merge)[0];
    Ok(
        match dest
            .map(|d| ctx.ssa.value_ty(d))
            .unwrap_or(SlotKind::Dynamic)
        {
            SlotKind::Int => Out::Native(res),
            SlotKind::Float
            | SlotKind::Bool
            | SlotKind::Str
            | SlotKind::Ref
            | SlotKind::Dynamic => Out::Boxed(box_int(b, res)),
        },
    )
}

pub(super) fn emit_set(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    object: u32,
    index: u32,
    value: u32,
) -> Result<(), String> {
    let lay = ctx.helpers.buffer_layout;
    let kind_off = ctx.helpers.array_layout.kind_off as i32;
    let exec_ctx = heap::exec_ctx(ctx);
    let key = load_value(b, ctx, values, index)?;

    let slow = b.create_block();
    b.set_cold_block(slow);
    let merge = b.create_block();

    let m = cranelift_codegen::ir::MemFlagsData::trusted();
    if ctx.ssa.value_ty(value) == SlotKind::Int {
        let raw = load_value(b, ctx, values, value)?;
        let obj = heap::boxed_value(b, ctx, values, object)?;
        let (obj_tag, obj_payload) = b.ins().isplit(obj);
        let (data, len) = match ctx.views.of(object) {
            Some(v) => {
                let data = b.use_var(v.data);
                let cached_len = b.use_var(v.len);
                let cached_disc = b.use_var(v.disc);
                let ready = b.create_block();
                b.append_block_param(ready, types::I64);
                b.append_block_param(ready, types::I64);
                let miss = b.create_block();
                b.ins()
                    .brif(data, ready, &[data.into(), cached_len.into()], miss, &[]);
                let _ = cached_disc;
                b.switch_to_block(miss);
                let (d, l) = resolve_parts(b, &lay, kind_off, obj_tag, obj_payload, slow);
                b.def_var(v.data, d);
                b.def_var(v.len, l);
                let disc = b.ins().iconst(types::I64, BYTES_VIEW_DISC);
                b.def_var(v.disc, disc);
                super::store::drop_home_addrs(ctx);
                b.ins().jump(ready, &[d.into(), l.into()]);
                b.switch_to_block(ready);
                let p = b.block_params(ready);
                (p[0], p[1])
            }
            None => resolve_parts(b, &lay, kind_off, obj_tag, obj_payload, slow),
        };
        let byte = b.ins().band_imm_u(raw, 0xFF);
        let in_bounds = b.ins().icmp(IntCC::UnsignedLessThan, key, len);
        let hit = b.create_block();
        b.ins().brif(in_bounds, hit, &[], slow, &[]);
        b.switch_to_block(hit);
        let at = b.ins().iadd(data, key);
        b.ins().istore8(m, byte, at, 0);
        b.ins().jump(merge, &[]);
    } else {
        b.ins().jump(slow, &[]);
    }

    b.switch_to_block(slow);
    let obj = heap::boxed_value(b, ctx, values, object)?;
    let (ot, op) = b.ins().isplit(obj);
    let boxed_key = box_int(b, key);
    let (kt, kp) = b.ins().isplit(boxed_key);
    let (vt, vp) = heap::boxed_parts(b, ctx, values, value)?;
    crate::clif::from_ssa::exceptions::call_checked(
        b,
        ctx,
        ctx.helpers.jit_array_set_fast,
        &[exec_ctx, ot, op, kt, kp, vt, vp],
    )?;
    b.ins().jump(merge, &[]);

    b.switch_to_block(merge);
    Ok(())
}

fn resolve_parts(
    b: &mut FunctionBuilder,
    lay: &JitBufferLayout,
    kind_off: i32,
    obj_tag: Value,
    obj_payload: Value,
    slow: Block,
) -> (Value, Value) {
    let m = cranelift_codegen::ir::MemFlagsData::trusted();
    let kind = b.ins().band_imm_u(obj_tag, KIND_MASK);
    let is_heap = b.ins().icmp_imm_u(IntCC::Equal, kind, HEAP_KIND);
    let chk = b.create_block();
    b.ins().brif(is_heap, chk, &[], slow, &[]);
    b.switch_to_block(chk);
    let slot = obj_payload;
    let tagb = b.ins().uload8(types::I64, m, slot, kind_off);
    let is_buf = b
        .ins()
        .icmp_imm_u(IntCC::Equal, tagb, lay.buffer_tag as i64);
    let ok = b.create_block();
    b.ins().brif(is_buf, ok, &[], slow, &[]);
    b.switch_to_block(ok);
    let base = lay.payload_off as i32;
    let rc = b
        .ins()
        .load(types::I64, m, slot, base + lay.buf_rc_off as i32);
    let rc_off = b.ins().iconst(types::I64, lay.rc_value_off as i64);
    let val = b.ins().iadd(rc, rc_off);
    let vec_ptr = b.ins().load(types::I64, m, val, lay.vec_ptr_off as i32);
    let off = b.ins().uload32(m, slot, base + lay.buf_offset_off as i32);
    let len = b.ins().uload32(m, slot, base + lay.buf_len_off as i32);
    let data = b.ins().iadd(vec_ptr, off);
    (data, len)
}
