use cranelift_codegen::ir::{condcodes::IntCC, types, InstBuilder, Value};
use cranelift_frontend::FunctionBuilder;
use varn_types::register_meta::SlotKind;

use super::super::emit::{box_int, unbox_int, HEAP_KIND, KIND_MASK};
use super::{heap, load_value, Ctx, Out};

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

    let m = cranelift_codegen::ir::MemFlagsData::trusted();
    let (obj_tag, obj_payload) = b.ins().isplit(obj);
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
    let in_bounds = b.ins().icmp(IntCC::UnsignedLessThan, key, len);
    let hit = b.create_block();
    b.ins().brif(in_bounds, hit, &[], slow, &[]);
    b.switch_to_block(hit);
    let off_key = b.ins().iadd(off, key);
    let at = b.ins().iadd(vec_ptr, off_key);
    let v = b.ins().uload8(types::I64, m, at, 0);
    b.ins().jump(merge, &[v.into()]);

    b.switch_to_block(slow);
    let boxed_key = box_int(b, key);
    let (kt, kp) = b.ins().isplit(boxed_key);
    crate::clif::from_ssa::exceptions::call_checked(
        b,
        ctx,
        ctx.helpers.jit_array_get_fast,
        &[exec_ctx, obj_tag, obj_payload, kt, kp],
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
