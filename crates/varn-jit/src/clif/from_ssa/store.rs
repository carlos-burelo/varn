use cranelift_codegen::ir::{types, Value};
use cranelift_frontend::FunctionBuilder;
use varn_types::register_meta::SlotKind;

use super::{heap, Ctx};

pub(super) enum Out {
    Native(Value),

    Boxed(Value),
}

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

pub(super) fn is_heap(kind: SlotKind) -> bool {
    matches!(kind, SlotKind::Str | SlotKind::Ref | SlotKind::Dynamic)
}

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

pub(super) fn drop_home_addrs(ctx: &Ctx<'_>) {
    ctx.home_addrs.borrow_mut().clear();
}

fn home_addr(b: &mut FunctionBuilder, ctx: &Ctx<'_>, reg: u32) -> Result<Value, String> {
    if let Some(&a) = ctx.home_addrs.borrow().get(&reg) {
        return Ok(a);
    }
    let a = homes(ctx)?.addr(b, reg as usize);
    ctx.home_addrs.borrow_mut().insert(reg, a);
    Ok(a)
}

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

pub(super) fn home_load(b: &mut FunctionBuilder, ctx: &Ctx<'_>, reg: u32) -> Result<Value, String> {
    let h = homes(ctx)?;
    let addr = home_addr(b, ctx, reg)?;
    Ok(h.load_at(b, addr, reg as usize))
}

pub(super) fn clif_ty(kind: SlotKind) -> Option<cranelift_codegen::ir::Type> {
    match kind {
        SlotKind::Int | SlotKind::Bool => Some(types::I64),
        SlotKind::Float => Some(types::F64),
        SlotKind::Str | SlotKind::Ref | SlotKind::Dynamic => Some(types::I128),
    }
}
