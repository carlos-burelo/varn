//! Where SSA values live and how an instruction's result gets there.
//!
//! One rule for the whole lowering: a **scalar** value is a CLIF register; a
//! **heap** value is its VM home, the GC root, so it survives a collection
//! that moves it. A scalar's home is only written where the interpreter will
//! read it: when a `try` region opens, for the values its landing pad reads
//! ([`super::exceptions`]). A captured variable is not an SSA value and
//! always lives in its home ([`super::closures`]). An OSR entry reads the
//! scalars live into its loop header from their homes, where the interpreter
//! left them; one the resumed body also redefines — an outer loop's counter,
//! say — is a Cranelift variable instead of a fixed CLIF value, so the
//! header merges the entry's value with the body's ([`Ctx::carried`]).
//!
//! An instruction emitter never stores its own result: it hands back an
//! [`Out`] saying which representation it produced, and [`land`] puts it
//! where the destination's class says it lives.

use cranelift_codegen::ir::{types, Value};
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
/// the CLIF map. A result nobody reads (`dest` is `None`) is dropped.
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
    define_scalar(b, ctx, values, d, native);
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

/// Define scalar value `v`: in its carried variable, or the CLIF map.
pub(super) fn define_scalar(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &mut [Option<Value>],
    v: u32,
    x: Value,
) {
    match ctx.carried.get(&v) {
        Some(var) => b.def_var(*var, x),
        None => values[v as usize] = Some(x),
    }
}

/// Read a value: scalars from the CLIF map, heap values from their home.
pub(super) fn load_value(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    v: u32,
) -> Result<Value, String> {
    let kind = ctx.ssa.value_ty(v);
    if is_heap(kind) {
        load_home_value(b, ctx, ctx.ssa.reg(v), kind)
    } else if let Some(var) = ctx.carried.get(&v) {
        Ok(b.use_var(*var))
    } else {
        get(values, v)
    }
}

/// Read `reg`'s home and unbox it into `kind`'s native representation.
pub(super) fn load_home_value(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    reg: u32,
    kind: SlotKind,
) -> Result<Value, String> {
    let boxed = use_heap(b, ctx, reg)?;
    heap::unbox_dest(b, kind, boxed)
}

/// Inline access to this activation's homes.
fn homes<'a>(ctx: &'a Ctx<'_>) -> Result<super::super::homes::Homes<'a>, String> {
    let frame = ctx
        .frame
        .as_ref()
        .ok_or("from_ssa: heap value without a frame")?;
    Ok(super::super::homes::Homes {
        exec_ctx: frame.exec_ctx,
        base: frame.base,
        layout: &frame.layout,
        offsets: &ctx.helpers.frame_layout,
    })
}

/// Write a boxed heap value to `reg`'s home (the GC root).
pub(super) fn def_heap(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    reg: u32,
    boxed: Value,
) -> Result<(), String> {
    homes(ctx)?.store(b, reg as usize, boxed);
    Ok(())
}

/// Read a boxed heap value back from `reg`'s home.
pub(super) fn use_heap(b: &mut FunctionBuilder, ctx: &Ctx<'_>, reg: u32) -> Result<Value, String> {
    Ok(homes(ctx)?.load(b, reg as usize))
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
