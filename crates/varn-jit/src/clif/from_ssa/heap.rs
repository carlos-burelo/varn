use cranelift_codegen::ir::{condcodes::IntCC, types, InstBuilder, Value};
use cranelift_frontend::FunctionBuilder;

use super::{load_value, Ctx};

use super::super::emit::{box_bool, box_f64, box_int, call_helper, unbox_f64_coerce, unbox_int};

pub(super) fn exec_ctx(ctx: &Ctx<'_>) -> Value {
    ctx.exec_ctx
}

pub(super) fn boxed_value(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    v: u32,
) -> Result<Value, String> {
    let x = load_value(b, ctx, values, v)?;
    Ok(box_native(b, ctx.ssa.value_ty(v), x))
}

pub(super) fn box_native(
    b: &mut FunctionBuilder,
    kind: varn_types::register_meta::SlotKind,
    x: Value,
) -> Value {
    use varn_types::register_meta::SlotKind;
    match kind {
        SlotKind::Int => box_int(b, x),
        SlotKind::Float => box_f64(b, x),
        SlotKind::Bool => box_bool(b, x),
        SlotKind::Str | SlotKind::Ref | SlotKind::Dynamic => x,
    }
}

pub(super) fn boxed_parts(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    v: u32,
) -> Result<(Value, Value), String> {
    let boxed = boxed_value(b, ctx, values, v)?;
    Ok(b.ins().isplit(boxed))
}

pub(super) fn unbox_dest(
    b: &mut FunctionBuilder,
    dest: varn_types::register_meta::SlotKind,
    boxed: Value,
) -> Result<Value, String> {
    Ok(match dest {
        varn_types::register_meta::SlotKind::Int => unbox_int(b, boxed),
        varn_types::register_meta::SlotKind::Float => unbox_f64_coerce(b, boxed),
        varn_types::register_meta::SlotKind::Bool => super::super::emit::unbox_bool(b, boxed),
        varn_types::register_meta::SlotKind::Str
        | varn_types::register_meta::SlotKind::Ref
        | varn_types::register_meta::SlotKind::Dynamic => boxed,
    })
}

pub(super) fn emit_is_array(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    operand: u32,
) -> Result<Value, String> {
    let (tag, payload) = boxed_parts(b, ctx, values, operand)?;
    let ectx = exec_ctx(ctx);
    Ok(call_helper(
        b,
        ctx.cc,
        ctx.helpers.is_array,
        &[ectx, tag, payload],
    ))
}

pub(super) fn emit_unary_boxed(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    operand: u32,
    helper: usize,
) -> Result<Value, String> {
    let (tag, payload) = boxed_parts(b, ctx, values, operand)?;
    let ectx = exec_ctx(ctx);
    super::exceptions::call_checked(b, ctx, helper, &[ectx, tag, payload])?;
    Ok(b.ins().load(
        types::I128,
        cranelift_codegen::ir::MemFlagsData::trusted(),
        ectx,
        ctx.helpers.jit_native_result_offset as i32,
    ))
}

pub(super) fn emit_str_concat(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    lhs: u32,
    rhs: u32,
) -> Result<Value, String> {
    let (at, ap) = boxed_parts(b, ctx, values, lhs)?;
    let (bt, bp) = boxed_parts(b, ctx, values, rhs)?;
    let ectx = exec_ctx(ctx);
    super::exceptions::call_checked(b, ctx, ctx.helpers.str_concat, &[ectx, at, ap, bt, bp])?;
    Ok(b.ins().load(
        types::I128,
        cranelift_codegen::ir::MemFlagsData::trusted(),
        ectx,
        ctx.helpers.jit_native_result_offset as i32,
    ))
}

pub(super) fn emit_build_str(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    parts: &[u32],
) -> Result<Value, String> {
    let vals: Vec<Value> = parts
        .iter()
        .map(|v| boxed_value(b, ctx, values, *v))
        .collect::<Result<_, _>>()?;
    emit_window_boxed(b, ctx, ctx.helpers.build_str, &vals, vals.len())
}

pub(super) fn emit_build_array(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    elements: &[u32],
) -> Result<Value, String> {
    let vals: Vec<Value> = elements
        .iter()
        .map(|v| boxed_value(b, ctx, values, *v))
        .collect::<Result<_, _>>()?;
    emit_window_boxed(b, ctx, ctx.helpers.build_array_window, &vals, vals.len())
}

pub(super) fn emit_build_map(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    pairs: &[(u32, u32)],
) -> Result<Value, String> {
    let mut vals = Vec::with_capacity(pairs.len() * 2);
    for (k, v) in pairs {
        vals.push(boxed_value(b, ctx, values, *k)?);
        vals.push(boxed_value(b, ctx, values, *v)?);
    }
    emit_window_boxed(b, ctx, ctx.helpers.build_map_window, &vals, pairs.len())
}

pub(super) fn emit_build_object(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    ids: &[u32],
    keys: &[Box<str>],
    is_record: bool,
) -> Result<Value, String> {
    if is_record {
        return emit_build_object_slow(b, ctx, values, ids, keys, true);
    }
    let idx = ctx
        .proto
        .chunk
        .constants
        .iter()
        .position(|e| match e {
            varn_types::PoolEntry::Shape(k) => {
                k.len() == keys.len() && k.iter().zip(keys).all(|(a, b)| a.as_ref() == b.as_ref())
            }
            varn_types::PoolEntry::Literal(_)
            | varn_types::PoolEntry::Function(_)
            | varn_types::PoolEntry::Layout(_) => false,
        })
        .ok_or("from_ssa: object shape not in pool")?;
    let shape = ctx
        .proto
        .resolved_shape(idx)
        .ok_or("from_ssa: unresolved object shape")?;
    let shape_ptr = std::rc::Rc::as_ptr(&shape) as usize;
    let count = ids.len();
    let total = ctx.helpers.object_layout.data_off + varn_types::ObjData::bytes_for(count);
    let Some(class_idx) = varn_types::cell::class_for(total) else {
        return emit_build_object_slow(b, ctx, values, ids, keys, false);
    };
    emit_build_object_inline(b, ctx, values, ids, keys, shape_ptr, count, class_idx)
}

#[allow(clippy::too_many_arguments)]
fn emit_build_object_inline(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    ids: &[u32],
    keys: &[Box<str>],
    shape_ptr: usize,
    count: usize,
    class_idx: usize,
) -> Result<Value, String> {
    use varn_types::cell::{
        CELL_CLASSES, LANE_BUMP_OFF, LANE_END_OFF, SIZE_CLASS_LANE_OFF, SIZE_CLASS_STRIDE,
        VEC_LEN_OFF, VEC_PTR_OFF,
    };
    use varn_types::vm_value::KIND_HEAP;
    let ia = &ctx.helpers.instance_alloc;
    let cell_bytes = CELL_CLASSES[class_idx] as i64;
    let flags = cranelift_codegen::ir::MemFlagsData::trusted();
    let ectx = exec_ctx(ctx);

    let vals: Vec<Value> = ids
        .iter()
        .map(|v| boxed_value(b, ctx, values, *v))
        .collect::<Result<_, _>>()?;

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

    let born_base = b
        .ins()
        .iadd_imm_u(rcbox, (ia.young_off + ia.born_off) as i64);
    let born_ptr = b
        .ins()
        .load(types::I64, flags, born_base, VEC_PTR_OFF as i32);
    let born_len = b
        .ins()
        .load(types::I64, flags, born_base, VEC_LEN_OFF as i32);
    let born_cap = b.ins().load(
        types::I64,
        flags,
        born_base,
        varn_types::cell::VEC_CAP_OFF as i32,
    );

    let slow_blk = b.create_block();
    b.set_cold_block(slow_blk);
    let free_hit_blk = b.create_block();
    let free_pop_blk = b.create_block();
    let bump_blk = b.create_block();
    let bump_ok_blk = b.create_block();
    let header_blk = b.create_block();
    let born_ok_blk = b.create_block();
    let join_blk = b.create_block();
    b.append_block_param(header_blk, types::I64);
    b.append_block_param(join_blk, types::I128);

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
    let bump = b.ins().load(types::I64, flags, lane, LANE_BUMP_OFF as i32);
    let end = b.ins().load(types::I64, flags, lane, LANE_END_OFF as i32);

    let guard = b.ins().bor(c_hot, c_nat);
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
    emit_build_object_body(b, ctx, &vals, shape_ptr, count, class_idx, r);

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
    let slow_v = emit_build_object_slow(b, ctx, values, ids, keys, false)?;
    b.ins().jump(join_blk, &[slow_v.into()]);

    b.switch_to_block(join_blk);
    Ok(b.block_params(join_blk)[0])
}

#[allow(clippy::too_many_arguments)]
fn emit_build_object_body(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    vals: &[Value],
    shape_ptr: usize,
    count: usize,
    class_idx: usize,
    r: Value,
) {
    let olay = &ctx.helpers.object_layout;
    let flags = cranelift_codegen::ir::MemFlagsData::trusted();
    let header_imm = 3i64 | ((olay.object_tag as i64) << 8) | ((class_idx as i64) << 16);
    let header_v = b.ins().iconst(types::I64, header_imm);
    b.ins().store(flags, header_v, r, 0);
    let body = b.ins().iadd_imm_u(r, varn_types::cell::HEADER_BYTES as i64);
    let data = b.ins().iadd_imm_u(r, olay.data_off as i64);
    let zero64 = b.ins().iconst(types::I64, 0);
    let slot_bytes = olay.data_off - varn_types::cell::HEADER_BYTES;
    let mut zoff = 8usize;
    while zoff + 8 <= slot_bytes {
        b.ins().store(flags, zero64, body, zoff as i32);
        zoff += 8;
    }
    if zoff < slot_bytes {
        if slot_bytes - zoff >= 4 {
            let zero32 = b.ins().iconst(types::I32, 0);
            b.ins().store(flags, zero32, body, zoff as i32);
            zoff += 4;
        }
        if slot_bytes - zoff >= 2 {
            let zero16 = b.ins().iconst(types::I16, 0);
            b.ins().store(flags, zero16, body, zoff as i32);
            zoff += 2;
        }
        if zoff < slot_bytes {
            let zero8 = b.ins().iconst(types::I8, 0);
            b.ins().store(flags, zero8, body, zoff as i32);
        }
    }
    let tag_v = b.ins().iconst(types::I8, olay.object_tag as i64);
    b.ins().store(flags, tag_v, body, 0);
    b.ins().store(
        flags,
        data,
        body,
        (olay.payload_off - varn_types::cell::HEADER_BYTES) as i32,
    );
    let len_meta = b.ins().iconst(types::I64, count as i64);
    b.ins()
        .store(flags, len_meta, r, olay.payload_len_off as i32);
    let shape_v = b.ins().iconst(types::I64, shape_ptr as i64);
    let rc_lay = &ctx.helpers.call_layout;
    let shape_ctl = b.ins().iadd_imm_s(shape_v, -(rc_lay.rc_value_off as i64));
    b.ins().store(flags, shape_ctl, data, olay.shape_off as i32);
    let strong = b
        .ins()
        .load(types::I64, flags, shape_ctl, rc_lay.rc_strong_off as i32);
    let strong1 = b.ins().iadd_imm_u(strong, 1);
    b.ins()
        .store(flags, strong1, shape_ctl, rc_lay.rc_strong_off as i32);
    let len_v = b.ins().iconst(types::I32, count as i64);
    b.ins().store(flags, len_v, data, olay.len_off as i32);
    let zero32 = b.ins().iconst(types::I32, 0);
    b.ins()
        .store(flags, zero32, data, (olay.len_off + 4) as i32);
    let zero64 = b.ins().iconst(types::I64, 0);
    b.ins()
        .store(flags, zero64, data, (olay.values_off - 8) as i32);
    for (i, v) in vals.iter().enumerate() {
        b.ins()
            .store(flags, *v, data, (olay.values_off + i * 16) as i32);
    }
}

fn emit_build_object_slow(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    ids: &[u32],
    keys: &[Box<str>],
    is_record: bool,
) -> Result<Value, String> {
    let idx = ctx
        .proto
        .chunk
        .constants
        .iter()
        .position(|e| match e {
            varn_types::PoolEntry::Shape(k) => {
                k.len() == keys.len() && k.iter().zip(keys).all(|(a, b)| a.as_ref() == b.as_ref())
            }
            varn_types::PoolEntry::Literal(_)
            | varn_types::PoolEntry::Function(_)
            | varn_types::PoolEntry::Layout(_) => false,
        })
        .ok_or("from_ssa: object shape not in pool")?;
    let shape = ctx
        .proto
        .resolved_shape(idx)
        .ok_or("from_ssa: unresolved object shape")?;
    let shape_ptr = std::rc::Rc::as_ptr(&shape) as usize;

    let vals: Vec<Value> = ids
        .iter()
        .map(|v| boxed_value(b, ctx, values, *v))
        .collect::<Result<_, _>>()?;
    let ectx = exec_ctx(ctx);
    let count = vals.len();
    let slot = b.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
        cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
        (count.max(1) * 16) as u32,
        4,
    ));
    let addr = b.ins().stack_addr(types::I64, slot, 0);
    for (i, v) in vals.iter().enumerate() {
        b.ins().store(
            cranelift_codegen::ir::MemFlagsData::trusted(),
            *v,
            addr,
            (i * 16) as i32,
        );
    }
    let count_v = b.ins().iconst(types::I64, count as i64);
    let shape_v = b.ins().iconst(types::I64, shape_ptr as i64);
    let rec_v = b.ins().iconst(types::I64, is_record as i64);
    super::exceptions::call_checked(
        b,
        ctx,
        ctx.helpers.build_object_window,
        &[ectx, addr, count_v, shape_v, rec_v],
    )?;
    Ok(b.ins().load(
        types::I128,
        cranelift_codegen::ir::MemFlagsData::trusted(),
        ectx,
        ctx.helpers.jit_native_result_offset as i32,
    ))
}

fn emit_window_boxed(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    helper: usize,
    vals: &[Value],
    count: usize,
) -> Result<Value, String> {
    let ectx = exec_ctx(ctx);
    let addr = super::call::scratch_addr(b, ctx, vals.len().max(1));
    for (i, v) in vals.iter().enumerate() {
        b.ins().store(
            cranelift_codegen::ir::MemFlagsData::trusted(),
            *v,
            addr,
            (i * 16) as i32,
        );
    }
    let count_v = b.ins().iconst(types::I64, count as i64);
    super::exceptions::call_checked(b, ctx, helper, &[ectx, addr, count_v])?;
    Ok(b.ins().load(
        types::I128,
        cranelift_codegen::ir::MemFlagsData::trusted(),
        ectx,
        ctx.helpers.jit_native_result_offset as i32,
    ))
}
