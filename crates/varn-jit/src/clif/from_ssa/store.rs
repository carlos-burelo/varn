//! Where SSA values live and how an instruction's result gets there.
//!
//! One rule for the whole lowering: every SSA value is a CLIF value. A heap
//! value (`Str`/`Ref`/`Dyn`, an `I128` `VmValue`) is declared to Cranelift as
//! a GC reference, so at every call it is spilled to a stack-map slot that
//! the collector reads and rewrites (`crate::stack_roots`) and reloaded after.
//!
//! A home is written only where the interpreter will read it — a `try`
//! landing pad ([`super::exceptions`]), a suspension point
//! ([`super::extra`]) — and read only where the interpreter left a value:
//! the arguments and `this` at entry, an OSR entry ([`super::osr`]), a
//! helper that lands its result there. A captured variable is not an SSA
//! value and always lives in its home ([`super::closures`]).

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
}

/// Define `dest` from `out`, converted to the destination's class. A result
/// nobody reads (`dest` is `None`) is dropped.
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
    let native = match out {
        Out::Native(v) => v,
        Out::Boxed(v) if is_heap(kind) => v,
        Out::Boxed(v) => heap::unbox_dest(b, kind, v)?,
    };
    define(b, ctx, values, d, native);
    Ok(())
}

/// Whether an SSA value is a boxed `VmValue` the collector must see.
pub(super) fn is_heap(kind: SlotKind) -> bool {
    matches!(kind, SlotKind::Str | SlotKind::Ref | SlotKind::Dynamic)
}

/// Define value `v`: in its carried variable, or the CLIF map. A heap value
/// becomes a GC root for every call it lives across.
pub(super) fn define(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &mut [Option<Value>],
    v: u32,
    x: Value,
) {
    match ctx.carried.get(&v) {
        Some(var) => b.def_var(*var, x),
        None => {
            if is_heap(ctx.ssa.value_ty(v)) {
                b.declare_value_needs_stack_map(x);
            }
            values[v as usize] = Some(x);
        }
    }
}

/// Read value `v` in its class's native representation.
pub(super) fn load_value(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    v: u32,
) -> Result<Value, String> {
    if let Some(var) = ctx.carried.get(&v) {
        return Ok(b.use_var(*var));
    }
    values
        .get(v as usize)
        .copied()
        .flatten()
        .ok_or_else(|| format!("from_ssa: value {v} used before definition"))
}

/// Read `reg`'s home and unbox it into `kind`'s native representation.
pub(super) fn load_home_value(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    reg: u32,
    kind: SlotKind,
) -> Result<Value, String> {
    let boxed = home_load(b, ctx, reg)?;
    if is_heap(kind) {
        Ok(boxed)
    } else {
        heap::unbox_dest(b, kind, boxed)
    }
}

/// Inline access to this activation's homes.
fn homes<'a>(ctx: &'a Ctx<'_>) -> Result<super::super::homes::Homes<'a>, String> {
    let frame = ctx
        .frame
        .as_ref()
        .ok_or("from_ssa: home access without a frame")?;
    Ok(super::super::homes::Homes {
        exec_ctx: frame.exec_ctx,
        base: frame.base.ok_or(super::NEEDS_ACTIVATION)?,
        layout: &frame.layout,
        offsets: &ctx.helpers.frame_layout,
    })
}

/// Drop every memoized home address. Required whenever emission leaves the
/// straight-line flow for an arm that does not dominate the join: an
/// address memoized inside a slow/miss arm is garbage everywhere the arm
/// does not dominate, and the verifier rejects the use (debug) or the
/// backend miscompiles it (release). Main-flow entries are unaffected —
/// they dominate everything below them — so this is only called where a
/// join can be reached around the fill.
pub(super) fn drop_home_addrs(ctx: &Ctx<'_>) {
    ctx.home_addrs.borrow_mut().clear();
}

/// Machine address of `reg`'s home, memoized within the current block: the
/// FrameStore vectors only reallocate when a frame is pushed (a real call),
/// so between may-push points the same address Value serves every access
/// and Cranelift folds what recomputation kept separate. The driver clears
/// the map at each block (cross-block reuse would need a dominance proof)
/// and after any instruction that may push a frame (see
/// [`super::may_push_frame`]).
fn home_addr(b: &mut FunctionBuilder, ctx: &Ctx<'_>, reg: u32) -> Result<Value, String> {
    if let Some(&a) = ctx.home_addrs.borrow().get(&reg) {
        return Ok(a);
    }
    let a = homes(ctx)?.addr(b, reg as usize);
    ctx.home_addrs.borrow_mut().insert(reg, a);
    Ok(a)
}

/// Write a boxed value to `reg`'s home, where the interpreter reads it.
pub(super) fn home_store(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    reg: u32,
    boxed: Value,
) -> Result<(), String> {
    let h = homes(ctx)?;
    let addr = home_addr(b, ctx, reg)?;
    h.store_at(b, addr, reg as usize, boxed);
    Ok(())
}

/// Read a boxed value from `reg`'s home, where the interpreter left it.
pub(super) fn home_load(b: &mut FunctionBuilder, ctx: &Ctx<'_>, reg: u32) -> Result<Value, String> {
    let h = homes(ctx)?;
    let addr = home_addr(b, ctx, reg)?;
    Ok(h.load_at(b, addr, reg as usize))
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
