//! Heap operations of the SSA lowering: the boxed runtime helpers for strings,
//! aggregates and type questions, plus the shared boxing helpers.
//!
//! Every one needs the live `exec_ctx` (they allocate and/or inspect the heap),
//! so they only appear in a frame-aware body. Operands are read from their
//! homes and results are boxed `VmValue`s; the caller lands a heap result in its
//! home (`def_heap`) or unboxes a scalar result. Because the homes are the GC
//! roots, an operand survives the helper's own allocation by construction — no
//! flush/reload list is needed.
//!
//! Property/index/field access lives in [`super::props`]; this file owns
//! constants, string/aggregate construction and type questions.

use cranelift_codegen::ir::{types, InstBuilder, MemFlags, Value};
use cranelift_frontend::FunctionBuilder;

use super::{load_value, Ctx};

use super::super::emit::{
    box_bool, box_f64, box_int, call_helper, call_helper_void, unbox_f64_coerce, unbox_int,
};

/// The live `ExecCtx` of a frame-aware body; heap ops have no leaf form.
pub(super) fn exec_ctx(ctx: &Ctx<'_>) -> Result<Value, String> {
    ctx.frame
        .as_ref()
        .map(|f| f.exec_ctx)
        .ok_or_else(|| "from_ssa: heap op without a frame".into())
}

/// A value as a boxed `VmValue` (`I128`): scalars are boxed by their class,
/// heap values are already one.
pub(super) fn boxed_value(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    v: u32,
) -> Result<Value, String> {
    let x = load_value(b, ctx, values, v)?;
    Ok(match ctx.ssa.value_ty(v) {
        varn_types::register_meta::SlotKind::Int => box_int(b, x),
        varn_types::register_meta::SlotKind::Float => box_f64(b, x),
        varn_types::register_meta::SlotKind::Bool => box_bool(b, x),
        _ => x,
    })
}

/// A value split into its `(tag, payload)` halves, boxing scalars first.
pub(super) fn boxed_parts(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    v: u32,
) -> Result<(Value, Value), String> {
    let boxed = boxed_value(b, ctx, values, v)?;
    Ok(b.ins().isplit(boxed))
}

/// Unbox a helper's boxed result into the class `dest` expects.
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

/// `IsArray x` — a `bool` result (no allocation).
pub(super) fn emit_is_array(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    operand: u32,
) -> Result<Value, String> {
    let (tag, payload) = boxed_parts(b, ctx, values, operand)?;
    let ectx = exec_ctx(ctx)?;
    Ok(call_helper(
        b,
        ctx.cc,
        ctx.helpers.is_array,
        &[ectx, tag, payload],
    ))
}

/// `helper(ctx, value) -> VmValue` — the result lands in `jit_native_result`.
pub(super) fn emit_unary_boxed(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    operand: u32,
    helper: usize,
) -> Result<Value, String> {
    let (tag, payload) = boxed_parts(b, ctx, values, operand)?;
    let ectx = exec_ctx(ctx)?;
    call_helper_void(b, ctx.cc, helper, &[ectx, tag, payload]);
    Ok(b.ins().load(
        types::I128,
        MemFlags::trusted(),
        ectx,
        ctx.helpers.jit_native_result_offset as i32,
    ))
}

/// `StrConcat a b` — a heap string result. Operands are boxed first: a mixed
/// `"a" + 1` has a scalar operand the helper stringifies.
pub(super) fn emit_str_concat(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    lhs: u32,
    rhs: u32,
) -> Result<Value, String> {
    let (at, ap) = boxed_parts(b, ctx, values, lhs)?;
    let (bt, bp) = boxed_parts(b, ctx, values, rhs)?;
    let ectx = exec_ctx(ctx)?;
    call_helper_void(b, ctx.cc, ctx.helpers.str_concat, &[ectx, at, ap, bt, bp]);
    Ok(b.ins().load(
        types::I128,
        MemFlags::trusted(),
        ectx,
        ctx.helpers.jit_native_result_offset as i32,
    ))
}

/// `BuildStr parts…` — a heap string result, from a native window.
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

/// `BuildArray elems…` — a heap array result, from a native window.
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

/// `BuildMap k0 v0 …` — a heap map result, from a native window.
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

/// `BuildObject`/`BuildRecord` — a heap result. The shape is resolved from the
/// proto's pool by key match (the same keys the bytecode `add_shape` used), so
/// the baked `Shape` pointer is the one the runtime already interned.
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
    let ectx = exec_ctx(ctx)?;
    let count = vals.len();
    let slot = b.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
        cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
        (count.max(1) * 16) as u32,
        4,
    ));
    let addr = b.ins().stack_addr(types::I64, slot, 0);
    for (i, v) in vals.iter().enumerate() {
        b.ins()
            .store(MemFlags::trusted(), *v, addr, (i * 16) as i32);
    }
    let count_v = b.ins().iconst(types::I64, count as i64);
    let shape_v = b.ins().iconst(types::I64, shape_ptr as i64);
    let rec_v = b.ins().iconst(types::I64, is_record as i64);
    // A value may be a closure whose upvalues must be closed; scanning is
    // always safe (a non-closure is skipped).
    let mhc_v = b.ins().iconst(types::I64, 1);
    call_helper_void(
        b,
        ctx.cc,
        ctx.helpers.build_object_window,
        &[ectx, addr, count_v, shape_v, rec_v, mhc_v],
    );
    Ok(b.ins().load(
        types::I128,
        MemFlags::trusted(),
        ectx,
        ctx.helpers.jit_native_result_offset as i32,
    ))
}

/// Call a `helper(ctx, ptr, count)` void helper with `vals` staged as a boxed
/// window, returning its `jit_native_result`.
fn emit_window_boxed(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    helper: usize,
    vals: &[Value],
    count: usize,
) -> Result<Value, String> {
    let ectx = exec_ctx(ctx)?;
    let slot = b.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
        cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
        (vals.len().max(1) * 16) as u32,
        4,
    ));
    let addr = b.ins().stack_addr(types::I64, slot, 0);
    for (i, v) in vals.iter().enumerate() {
        b.ins()
            .store(MemFlags::trusted(), *v, addr, (i * 16) as i32);
    }
    let count_v = b.ins().iconst(types::I64, count as i64);
    call_helper_void(b, ctx.cc, helper, &[ectx, addr, count_v]);
    Ok(b.ins().load(
        types::I128,
        MemFlags::trusted(),
        ectx,
        ctx.helpers.jit_native_result_offset as i32,
    ))
}
