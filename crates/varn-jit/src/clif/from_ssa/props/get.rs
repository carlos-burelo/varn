use cranelift_codegen::ir::{condcodes::IntCC, types, InstBuilder, Value};
use cranelift_frontend::FunctionBuilder;

use super::super::super::emit::{call_helper_void, HEAP_KIND, KIND_MASK};
use super::super::heap::boxed_parts;
use super::super::store::drop_home_addrs;
use super::super::Ctx;
use super::shared::str_idx;

pub(crate) fn emit_get_property(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    object: u32,
    name: &str,
    cs: u16,
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

    let m = cranelift_codegen::ir::MemFlagsData::trusted();
    let olay = &ctx.helpers.object_layout;
    let alay = &ctx.helpers.array_layout;
    let heap_off = ctx.helpers.heap_field_offset;

    let kind = b.ins().band_imm_u(ot, KIND_MASK);
    let is_heap = b.ins().icmp_imm_u(IntCC::Equal, kind, HEAP_KIND);
    let chk = b.create_block();
    b.ins().brif(is_heap, chk, &[], slow, &[]);
    b.switch_to_block(chk);

    let raw = b.ins().band_imm_u(op, 0xFFFF_FFFF);
    let rc = b.ins().load(types::I64, m, ectx, heap_off as i32);
    let old_bit = b.ins().band_imm_u(raw, 0x8000_0000);
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
    let idx_old = b.ins().band_imm_u(raw, 0x7FFF_FFFF);
    let base = b.ins().select(old_bit, base_old, base_nur);
    let idx = b.ins().select(old_bit, idx_old, raw);
    let byte_off = b.ins().imul_imm_u(idx, alay.slot_size as i64);
    let slot_addr = b.ins().iadd(base, byte_off);

    let tagb = b.ins().uload8(types::I64, m, slot_addr, 0);
    let is_obj = b
        .ins()
        .icmp_imm_u(IntCC::Equal, tagb, olay.object_tag as i64);
    let ok = b.create_block();
    b.ins().brif(is_obj, ok, &[], slow, &[]);
    b.switch_to_block(ok);

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
    let values_base = b.ins().iadd_imm_u(data_ptr, olay.values_off as i64);

    let ic_base = b.ins().load(
        types::I64,
        m,
        frame.closure,
        ctx.helpers.closure_ic_entries_offset as i32,
    );
    let slot_base = b.ins().iadd_imm_u(
        ic_base,
        (cs as i64) * (ctx.helpers.poly_ic_slot_size as i64),
    );
    let mut next = b.create_block();
    b.ins().jump(next, &[]);
    for i in 0..8 {
        b.switch_to_block(next);
        next = b.create_block();
        let hit = b.create_block();
        let entry = b.ins().iadd_imm_u(
            slot_base,
            (i * std::mem::size_of::<varn_types::chunk::CacheEntry>()) as i64,
        );
        let id32 = b.ins().load(types::I32, m, entry, 0);
        let id = b.ins().uextend(types::I64, id32);
        let kc = b.ins().uload8(types::I64, m, entry, 6);
        let id_eq = b.ins().icmp(IntCC::Equal, id, shape_id);
        let kind_ok = b.ins().icmp_imm_u(
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
        let off = b.ins().ishl_imm_u(slot16, 4);
        let addr = b.ins().iadd(values_base, off);
        let val = b.ins().load(types::I128, m, addr, 0);
        b.ins().jump(merge, &[val.into()]);
    }
    b.switch_to_block(next);
    b.ins().jump(slow, &[]);

    b.switch_to_block(slow);
    let name_v = b.ins().iconst(types::I64, name_idx as i64);
    let cs_v = b.ins().iconst(types::I64, cs as i64);
    let ip_v = b.ins().iconst(types::I64, 0);
    call_helper_void(
        b,
        ctx.cc,
        ctx.helpers.get_property_flat,
        &[ectx, frame.closure, ot, op, name_v, cs_v, ip_v],
    );
    drop_home_addrs(ctx);
    let slow_val = b.ins().load(
        types::I128,
        cranelift_codegen::ir::MemFlagsData::trusted(),
        ectx,
        ctx.helpers.jit_native_result_offset as i32,
    );
    b.ins().jump(merge, &[slow_val.into()]);

    b.switch_to_block(merge);
    Ok(b.block_params(merge)[0])
}
