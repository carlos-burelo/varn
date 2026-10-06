












use cranelift_codegen::ir::{types, InstBuilder, Value};
use cranelift_frontend::FunctionBuilder;

use super::{load_value, Ctx};

use super::super::emit::{
    box_bool, box_f64, box_int, call_helper, call_helper_void, unbox_f64_coerce, unbox_int,
};



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
        _ => boxed,
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
    call_helper_void(b, ctx.cc, helper, &[ectx, tag, payload]);
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
    call_helper_void(b, ctx.cc, ctx.helpers.str_concat, &[ectx, at, ap, bt, bp]);
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
    let idx = ctx
        .proto
        .chunk
        .constants
        .iter()
        .position(|e| match e {
            varn_types::PoolEntry::Shape(k) => {
                k.len() == keys.len() && k.iter().zip(keys).all(|(a, b)| a.as_ref() == b.as_ref())
            }
            _ => false,
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
    
    
    let mhc_v = b.ins().iconst(types::I64, 1);
    call_helper_void(
        b,
        ctx.cc,
        ctx.helpers.build_object_window,
        &[ectx, addr, count_v, shape_v, rec_v, mhc_v],
    );
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
    call_helper_void(b, ctx.cc, helper, &[ectx, addr, count_v]);
    Ok(b.ins().load(
        types::I128,
        cranelift_codegen::ir::MemFlagsData::trusted(),
        ectx,
        ctx.helpers.jit_native_result_offset as i32,
    ))
}
