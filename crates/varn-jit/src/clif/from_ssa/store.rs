//! Where SSA values live and how an instruction's result gets there.
//!
//! One rule for the whole lowering: a **scalar** value is a CLIF register (and,
//! in a frame-aware body, also its home); a **heap** value is its VM home,
//! the GC root. An instruction emitter never stores its own result: it hands
//! back an [`Out`] saying which representation it produced, and [`land`] puts
//! it where the destination's class says it lives.

use cranelift_codegen::ir::{types, InstBuilder, MemFlags, StackSlotData, StackSlotKind, Value};
use cranelift_frontend::FunctionBuilder;
use varn_types::register_meta::SlotKind;

use super::{heap, Ctx};

/// An instruction's result, as the emitter produced it.
pub(super) enum Out {
    /// In the destination class's native representation (`I64`/`F64`, or an
    /// `I128` `VmValue` for a heap destination).
    Native(Value),
    /// A whole `VmValue`, whatever the destination class.
    Boxed(Value),
    /// A `VmValue` the emitter's helper already wrote to the destination's
    /// home.
    Landed(Value),
}

/// Put `out` where `dest` lives: a heap destination in its home, a scalar in
/// the CLIF map (and its home, in a frame-aware body). A result nobody reads
/// (`dest` is `None`) is dropped.
pub(super) fn land(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &mut [Option<Value>],
    dest: Option<u32>,
    out: Out,
) -> Result<(), String> {
    let Some(d) = dest else {
        return Ok(());
    };
    let kind = ctx.ssa.value_ty(d);
    if is_heap(kind) {
        return match out {
            Out::Native(v) | Out::Boxed(v) => def_heap(b, ctx, ctx.ssa.reg(d), v),
            Out::Landed(_) => Ok(()),
        };
    }
    let native = match out {
        Out::Native(v) => v,
        Out::Boxed(v) | Out::Landed(v) => heap::unbox_dest(b, kind, v)?,
    };
    if ctx.homes_all {
        store_home_value(b, ctx, d, native)?;
    }
    values[d as usize] = Some(native);
    Ok(())
}

/// Whether an SSA value is stored in a home rather than a CLIF register.
pub(super) fn is_heap(kind: SlotKind) -> bool {
    matches!(kind, SlotKind::Str | SlotKind::Ref | SlotKind::Dynamic)
}

fn get(values: &[Option<Value>], v: u32) -> Result<Value, String> {
    values
        .get(v as usize)
        .copied()
        .flatten()
        .ok_or_else(|| format!("from_ssa: value {v} used before definition"))
}

/// Read a value: scalars from the CLIF map, heap values from their home. In a
/// frame-aware body every value lives in its home.
pub(super) fn load_value(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    v: u32,
) -> Result<Value, String> {
    let kind = ctx.ssa.value_ty(v);
    if ctx.homes_all || is_heap(kind) {
        load_home_value(b, ctx, ctx.ssa.reg(v), kind)
    } else {
        get(values, v)
    }
}

/// Read `reg`'s home and unbox it into `kind`'s native representation.
fn load_home_value(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    reg: u32,
    kind: SlotKind,
) -> Result<Value, String> {
    let boxed = use_heap(b, ctx, reg)?;
    heap::unbox_dest(b, kind, boxed)
}

/// Write a native scalar value to its home (boxing it), for a frame-aware body.
pub(super) fn store_home_value(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    value: u32,
    native: Value,
) -> Result<(), String> {
    let kind = ctx.ssa.value_ty(value);
    let boxed = match kind {
        SlotKind::Int => super::super::emit::box_int(b, native),
        SlotKind::Float => super::super::emit::box_f64(b, native),
        SlotKind::Bool => super::super::emit::box_bool(b, native),
        _ => native,
    };
    def_heap(b, ctx, ctx.ssa.reg(value), boxed)
}

/// Write a boxed heap value to `reg`'s home (the GC root).
pub(super) fn def_heap(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    reg: u32,
    boxed: Value,
) -> Result<(), String> {
    let frame = ctx
        .frame
        .as_ref()
        .ok_or("from_ssa: heap value without a frame")?;
    let (tag, payload) = b.ins().isplit(boxed);
    let reg_v = b.ins().iconst(types::I64, reg as i64);
    super::super::emit::call_helper_void(
        b,
        ctx.cc,
        ctx.helpers.home_store,
        &[frame.exec_ctx, frame.base, reg_v, tag, payload],
    );
    Ok(())
}

/// Read a boxed heap value back from `reg`'s home.
pub(super) fn use_heap(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    reg: u32,
) -> Result<Value, String> {
    let frame = ctx
        .frame
        .as_ref()
        .ok_or("from_ssa: heap value without a frame")?;
    let slot = b.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 16, 4));
    let addr = b.ins().stack_addr(types::I64, slot, 0);
    let reg_v = b.ins().iconst(types::I64, reg as i64);
    super::super::emit::call_helper_void(
        b,
        ctx.cc,
        ctx.helpers.home_load,
        &[frame.exec_ctx, frame.base, reg_v, addr],
    );
    Ok(b.ins().load(types::I128, MemFlags::trusted(), addr, 0))
}

/// CLIF type of an SSA value. `Bool` is a raw `I64` 0/1; `Ref`/`Dyn`/`Str` is a
/// 16-byte `VmValue` (`I128` = tag+payload).
pub(super) fn clif_ty(kind: SlotKind) -> Option<cranelift_codegen::ir::Type> {
    match kind {
        SlotKind::Int | SlotKind::Bool => Some(types::I64),
        SlotKind::Float => Some(types::F64),
        SlotKind::Str | SlotKind::Ref | SlotKind::Dynamic => Some(types::I128),
    }
}

