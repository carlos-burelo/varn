use cranelift_codegen::ir::{condcodes::IntCC, types, InstBuilder};
use cranelift_frontend::FunctionBuilder;
use varn_types::cell::{
    CELL_CLASSES, SIZE_CLASS_LANE_OFF, SIZE_CLASS_STRIDE, VEC_CAP_OFF, VEC_LEN_OFF, VEC_PTR_OFF,
};
use varn_types::vm_value::KIND_HEAP;

use super::heap::boxed_parts;
use super::props::str_idx;
use super::Ctx;

use super::super::emit::{box_null, call_helper_void};

pub(super) fn emit_make_class(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<cranelift_codegen::ir::Value>],
    name: &str,
    super_class: Option<u32>,
) -> Result<cranelift_codegen::ir::Value, String> {
    let frame = ctx
        .frame
        .as_ref()
        .ok_or("from_ssa: MakeClass without a frame")?;
    let name_idx = str_idx(ctx, name)?;
    let (st, sp) = match super_class {
        Some(v) => boxed_parts(b, ctx, values, v)?,
        None => {
            let null = box_null(b);
            b.ins().isplit(null)
        }
    };
    let ectx = frame.exec_ctx;
    let name_v = b.ins().iconst(types::I64, name_idx as i64);
    call_helper_void(
        b,
        ctx.cc,
        ctx.helpers.make_class,
        &[ectx, frame.closure, st, sp, name_v],
    );
    Ok(b.ins().load(
        types::I128,
        cranelift_codegen::ir::MemFlagsData::trusted(),
        ectx,
        ctx.helpers.jit_native_result_offset as i32,
    ))
}

pub(super) fn emit_alloc_instance(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<cranelift_codegen::ir::Value>],
    class: u32,
    payload_size: u32,
) -> Result<cranelift_codegen::ir::Value, String> {
    use varn_types::cell;
    let ia = &ctx.helpers.instance_alloc;
    let total_bytes = cell::instance_cell_bytes(ia.heap_obj_bytes, payload_size) as i64;
    let data_off = cell::instance_data_off(ia.heap_obj_bytes) as i64;
    let Some(class_idx) = cell::class_for(total_bytes as usize) else {
        return emit_alloc_instance_slow(b, ctx, values, class);
    };
    let cell_bytes = CELL_CLASSES[class_idx] as i64;

    let frame = ctx
        .frame
        .as_ref()
        .ok_or("from_ssa: AllocInstance without a frame")?;
    let ectx = frame.exec_ctx;
    let flags = cranelift_codegen::ir::MemFlagsData::trusted();
    let (_ct, cp) = boxed_parts(b, ctx, values, class)?;

    let rcbox = b.ins().load(
        types::I64,
        flags,
        ectx,
        ctx.helpers.heap_field_offset as i32,
    );
    let hotspot = b
        .ins()
        .load(types::I64, flags, rcbox, ia.hotspot_off as i32);
    let c_hot = b.ins().icmp_imm_u(IntCC::NotEqual, hotspot, 0);
    let cells = b.ins().iadd_imm_u(rcbox, ia.cells_off as i64);
    let native_b = b.ins().load(types::I8, flags, cells, ia.native_off as i32);
    let c_nat = b.ins().icmp_imm_u(IntCC::NotEqual, native_b, 0);
    let body_c = b.ins().iadd_imm_u(cp, 8);
    let tag_b = b.ins().load(types::I8, flags, body_c, 0);
    let c_tag = b
        .ins()
        .icmp_imm_u(IntCC::NotEqual, tag_b, ia.class_tag as i64);
    let rc = b
        .ins()
        .load(types::I64, flags, body_c, ia.class_ref_off as i32);
    let class_id = b.ins().load(types::I32, flags, rc, ia.class_id_off as i32);
    let classes_ptr = b.ins().load(
        types::I64,
        flags,
        cells,
        (ia.classes_off + VEC_PTR_OFF) as i32,
    );
    let lane = b.ins().iadd_imm_u(
        classes_ptr,
        (class_idx as i64) * (SIZE_CLASS_STRIDE as i64) + (SIZE_CLASS_LANE_OFF as i64),
    );
    let free = b.ins().load(types::I64, flags, lane, 0);
    let bump = b.ins().load(
        types::I64,
        flags,
        lane,
        varn_types::cell::LANE_BUMP_OFF as i32,
    );
    let end = b.ins().load(
        types::I64,
        flags,
        lane,
        varn_types::cell::LANE_END_OFF as i32,
    );
    let born_base = b
        .ins()
        .iadd_imm_u(rcbox, (ia.young_off + ia.born_off) as i64);
    let born_ptr = b
        .ins()
        .load(types::I64, flags, born_base, VEC_PTR_OFF as i32);
    let born_len = b
        .ins()
        .load(types::I64, flags, born_base, VEC_LEN_OFF as i32);
    let born_cap = b
        .ins()
        .load(types::I64, flags, born_base, VEC_CAP_OFF as i32);

    let slow_blk = b.create_block();
    let free_hit_blk = b.create_block();
    let free_pop_blk = b.create_block();
    let bump_blk = b.create_block();
    let bump_ok_blk = b.create_block();
    let header_blk = b.create_block();
    let zhead_blk = b.create_block();
    let zbody_blk = b.create_block();
    let zdone_blk = b.create_block();
    let born_ok_blk = b.create_block();
    let join_blk = b.create_block();
    b.append_block_param(header_blk, types::I64);
    b.append_block_param(zhead_blk, types::I64);
    b.append_block_param(join_blk, types::I128);
    for cold in [
        slow_blk,
        free_hit_blk,
        free_pop_blk,
        bump_blk,
        bump_ok_blk,
        header_blk,
        zhead_blk,
        zbody_blk,
        zdone_blk,
        born_ok_blk,
    ] {
        b.set_cold_block(cold);
    }

    let guard = b.ins().bor(c_hot, c_nat);
    let guard = b.ins().bor(guard, c_tag);
    let force_slow = std::env::var("VARN_ALLOC_SLOW").is_ok();
    if force_slow {
        b.ins().jump(slow_blk, &[]);
    } else {
        b.ins().brif(guard, slow_blk, &[], free_hit_blk, &[]);
    }

    b.switch_to_block(free_hit_blk);
    let has_free = b.ins().icmp_imm_u(IntCC::NotEqual, free, 0);
    b.ins().brif(has_free, free_pop_blk, &[], bump_blk, &[]);

    b.switch_to_block(free_pop_blk);
    let free_next = b.ins().load(types::I64, flags, free, 8);
    b.ins().store(flags, free_next, lane, 0);
    b.ins().jump(header_blk, &[free.into()]);

    b.switch_to_block(bump_blk);
    let bump2 = b.ins().iadd_imm_u(bump, cell_bytes);
    let no_room = b.ins().icmp(IntCC::UnsignedGreaterThan, bump2, end);
    b.ins().brif(no_room, slow_blk, &[], bump_ok_blk, &[]);

    b.switch_to_block(bump_ok_blk);
    b.ins().store(flags, bump2, lane, 8);
    b.ins().jump(header_blk, &[bump.into()]);

    b.switch_to_block(header_blk);
    let r = b.block_params(header_blk)[0];
    let header_imm = 3i64 | ((ia.instance_tag as i64) << 8) | ((class_idx as i64) << 16);
    let header_v = b.ins().iconst(types::I64, header_imm);
    b.ins().store(flags, header_v, r, 0);
    let data = b.ins().iadd_imm_u(r, data_off);
    let class_id64 = b.ins().uextend(types::I64, class_id);
    let pay_imm = b.ins().iconst(types::I64, payload_size as i64);
    let payload_hi = b.ins().ishl_imm_u(pay_imm, 32);
    let hdr = b.ins().bor(class_id64, payload_hi);
    b.ins().store(flags, hdr, data, 0);
    let zero64 = b.ins().iconst(types::I64, 0);
    let zero32 = b.ins().iconst(types::I32, 0);
    let zero16 = b.ins().iconst(types::I16, 0);
    let zero8 = b.ins().iconst(types::I8, 0);
    b.ins().jump(zhead_blk, &[zero64.into()]);

    let nwords = (payload_size / 8) as i64;
    b.switch_to_block(zhead_blk);
    let i = b.block_params(zhead_blk)[0];
    let more = b.ins().icmp_imm_u(IntCC::UnsignedLessThan, i, nwords);
    b.ins().brif(more, zbody_blk, &[], zdone_blk, &[]);

    b.switch_to_block(zbody_blk);
    let zoff = b.ins().ishl_imm_u(i, 3);
    let zaddr = b.ins().iadd(data, zoff);
    b.ins().store(flags, zero64, zaddr, 8);
    let i2 = b.ins().iadd_imm_u(i, 1);
    b.ins().jump(zhead_blk, &[i2.into()]);

    b.switch_to_block(zdone_blk);
    let mut rem_off = 8 + nwords * 8;
    let mut rem = payload_size % 8;
    if rem >= 4 {
        b.ins().store(flags, zero32, data, rem_off as i32);
        rem_off += 4;
        rem -= 4;
    }
    if rem >= 2 {
        b.ins().store(flags, zero16, data, rem_off as i32);
        rem_off += 2;
        rem -= 2;
    }
    if rem >= 1 {
        b.ins().store(flags, zero8, data, rem_off as i32);
    }
    let body = r;
    let body_plus = b.ins().iadd_imm_u(body, 8);
    let hob = ia.heap_obj_bytes as i64;
    let mut boff = 0i64;
    while boff + 8 <= hob {
        b.ins().store(flags, zero64, body_plus, boff as i32);
        boff += 8;
    }
    let mut brem = (hob - boff) as u32;
    let mut broff = boff;
    if brem >= 4 {
        b.ins().store(flags, zero32, body_plus, broff as i32);
        broff += 4;
        brem -= 4;
    }
    if brem >= 2 {
        b.ins().store(flags, zero16, body_plus, broff as i32);
        broff += 2;
        brem -= 2;
    }
    if brem >= 1 {
        b.ins().store(flags, zero8, body_plus, broff as i32);
    }
    let tag_v = b.ins().iconst(types::I8, ia.instance_tag as i64);
    b.ins().store(flags, tag_v, body_plus, 0);
    b.ins()
        .store(flags, data, body_plus, ia.instance_ref_off as i32);
    let full = b
        .ins()
        .icmp(IntCC::UnsignedGreaterThanOrEqual, born_len, born_cap);
    b.ins().brif(full, slow_blk, &[], born_ok_blk, &[]);

    b.switch_to_block(born_ok_blk);
    let slot_off = b.ins().ishl_imm_u(born_len, 3);
    let slot = b.ins().iadd(born_ptr, slot_off);
    b.ins().store(flags, r, slot, 0);
    let len1 = b.ins().iadd_imm_u(born_len, 1);
    b.ins().store(flags, len1, born_base, VEC_LEN_OFF as i32);
    let heap_tag = b.ins().iconst(types::I64, KIND_HEAP as i64);
    let boxed = b.ins().iconcat(heap_tag, r);
    b.ins().jump(join_blk, &[boxed.into()]);

    b.switch_to_block(slow_blk);
    let slow_v = emit_alloc_instance_slow(b, ctx, values, class)?;
    b.ins().jump(join_blk, &[slow_v.into()]);

    b.switch_to_block(join_blk);
    Ok(b.block_params(join_blk)[0])
}

fn emit_alloc_instance_slow(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<cranelift_codegen::ir::Value>],
    class: u32,
) -> Result<cranelift_codegen::ir::Value, String> {
    let frame = ctx
        .frame
        .as_ref()
        .ok_or("from_ssa: AllocInstance without a frame")?;
    let (ct, cp) = boxed_parts(b, ctx, values, class)?;
    let ectx = frame.exec_ctx;
    call_helper_void(b, ctx.cc, ctx.helpers.alloc_instance, &[ectx, ct, cp]);
    Ok(b.ins().load(
        types::I128,
        cranelift_codegen::ir::MemFlagsData::trusted(),
        ectx,
        ctx.helpers.jit_native_result_offset as i32,
    ))
}

pub(super) fn emit_declare_layout(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<cranelift_codegen::ir::Value>],
    class: u32,
    layout: &varn_core::layout::ClassLayout,
) -> Result<(), String> {
    let frame = ctx
        .frame
        .as_ref()
        .ok_or("from_ssa: DeclareLayout without a frame")?;
    let (ct, cp) = boxed_parts(b, ctx, values, class)?;
    let layout_idx = ctx
        .proto
        .chunk
        .constants
        .iter()
        .position(|e| matches!(e, varn_types::PoolEntry::Layout(l) if l.as_ref() == layout))
        .ok_or("from_ssa: class layout not in pool")?;
    let ectx = frame.exec_ctx;
    let idx_v = b.ins().iconst(types::I64, layout_idx as i64);
    call_helper_void(
        b,
        ctx.cc,
        ctx.helpers.declare_layout,
        &[ectx, frame.closure, ct, cp, idx_v],
    );
    Ok(())
}

pub(super) fn emit_define_member(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<cranelift_codegen::ir::Value>],
    class: u32,
    name: &str,
    member: u32,
    kind: i64,
) -> Result<(), String> {
    let frame = ctx
        .frame
        .as_ref()
        .ok_or("from_ssa: class member op without a frame")?;
    let (ct, cp) = boxed_parts(b, ctx, values, class)?;
    let (mt, mp) = boxed_parts(b, ctx, values, member)?;
    let name_idx = str_idx(ctx, name)?;
    let ectx = frame.exec_ctx;

    let slot = b.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
        cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
        48,
        3,
    ));
    b.ins().stack_store(types::I64, ct, slot, 0);
    b.ins().stack_store(types::I64, cp, slot, 8);
    b.ins().stack_store(types::I64, mt, slot, 16);
    b.ins().stack_store(types::I64, mp, slot, 24);
    let name_v = b.ins().iconst(types::I64, name_idx as i64);
    b.ins().stack_store(types::I64, name_v, slot, 32);
    let kind_v = b.ins().iconst(types::I64, kind);
    b.ins().stack_store(types::I64, kind_v, slot, 40);
    let args = b.ins().stack_addr(types::I64, slot, 0);

    call_helper_void(
        b,
        ctx.cc,
        ctx.helpers.class_member_op,
        &[ectx, frame.closure, args],
    );
    Ok(())
}

pub(super) fn emit_get_super(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    name: &str,
) -> Result<cranelift_codegen::ir::Value, String> {
    let frame = ctx
        .frame
        .as_ref()
        .ok_or("from_ssa: GetSuper without a frame")?;
    frame.base.ok_or(super::NEEDS_ACTIVATION)?;
    let name_idx = str_idx(ctx, name)?;
    let ectx = frame.exec_ctx;
    let name_v = b.ins().iconst(types::I64, name_idx as i64);
    call_helper_void(b, ctx.cc, ctx.helpers.get_super, &[ectx, name_v]);
    Ok(b.ins().load(
        types::I128,
        cranelift_codegen::ir::MemFlagsData::trusted(),
        ectx,
        ctx.helpers.jit_native_result_offset as i32,
    ))
}

pub(super) fn emit_make_enum_variant(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    tag: i64,
    meta: &str,
) -> Result<cranelift_codegen::ir::Value, String> {
    let frame = ctx
        .frame
        .as_ref()
        .ok_or("from_ssa: MakeEnumVariant without a frame")?;
    let meta_v = b.ins().iconst(types::I64, str_idx(ctx, meta)? as i64);
    let tag_v = b.ins().iconst(types::I64, tag);
    call_helper_void(
        b,
        ctx.cc,
        ctx.helpers.make_enum_variant_const,
        &[frame.exec_ctx, frame.closure, tag_v, meta_v],
    );
    Ok(b.ins().load(
        types::I128,
        cranelift_codegen::ir::MemFlagsData::trusted(),
        frame.exec_ctx,
        ctx.helpers.jit_native_result_offset as i32,
    ))
}
