use super::super::super::emit::{call_helper, call_helper_void};
use super::super::heap::{boxed_value, exec_ctx};
use super::super::load_value;
use super::super::store::{home_store, Out};
use super::super::Ctx;
use super::extra_shared::{frame, native};
use cranelift_codegen::ir::{types, InstBuilder, Value};
use cranelift_frontend::FunctionBuilder;

pub(super) fn await_(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &mut [Option<Value>],
    operand: u32,
    resume_ip: u32,
    live: &[u32],
    dest: Option<u32>,
) -> Result<Option<Option<Out>>, String> {
    let ectx = exec_ctx(ctx);
    let h = ctx.helpers;
    let d = dest.ok_or("from_ssa: await without dest")?;
    spill(b, ctx, values, live)?;
    let v = boxed_value(b, ctx, values, operand)?;
    let (t, p) = b.ins().isplit(v);
    let dv = b.ins().iconst(types::I64, i64::from(ctx.ssa.reg(d)));
    let rv = b.ins().iconst(types::I64, i64::from(resume_ip));

    call_helper_void(b, ctx.cc, h.await_helper, &[ectx, t, p, dv, rv]);
    Ok(Some(None))
}

pub(super) fn spawn(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &mut [Option<Value>],
    operand: u32,
) -> Result<Option<Option<Out>>, String> {
    let ectx = exec_ctx(ctx);
    let h = ctx.helpers;
    let v = boxed_value(b, ctx, values, operand)?;
    let (t, p) = b.ins().isplit(v);
    call_helper_void(b, ctx.cc, h.spawn, &[ectx, t, p]);
    Ok(Some(Some(Out::Boxed(native(b, ectx, h)))))
}

pub(super) fn yield_(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &mut [Option<Value>],
    operand: u32,
    resume_ip: u32,
    live: &[u32],
    dest: Option<u32>,
) -> Result<Option<Option<Out>>, String> {
    let ectx = exec_ctx(ctx);
    let h = ctx.helpers;
    let d = dest.ok_or("from_ssa: yield without dest")?;
    spill(b, ctx, values, live)?;
    let v = boxed_value(b, ctx, values, operand)?;
    let (t, p) = b.ins().isplit(v);
    let dv = b.ins().iconst(types::I64, i64::from(ctx.ssa.reg(d)));
    let rv = b.ins().iconst(types::I64, i64::from(resume_ip));

    call_helper_void(b, ctx.cc, h.yield_helper, &[ectx, t, p, dv, rv]);
    Ok(Some(None))
}

pub(super) fn dispose(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    var: u32,
    is_await: bool,
    cs: u16,
) -> Result<Option<Option<Out>>, String> {
    let ectx = exec_ctx(ctx);
    frame(ctx)?;
    let reg = ctx
        .ssa
        .captured_reg(var)
        .ok_or("from_ssa: dispose unknown var")?;
    let recv = super::super::store::home_load(b, ctx, reg)?;
    let name = if is_await { "disposeAsync" } else { "dispose" };
    let niv = b
        .ins()
        .iconst(types::I64, super::super::props::str_idx(ctx, name)? as i64);
    let csv = b.ins().iconst(types::I64, i64::from(cs));
    let w = super::super::call::boxed_window(b, ctx, values, recv, &[])?;
    let total = b.ins().iconst(types::I64, 1);
    let out = super::super::call::entry_out_slot(b);
    let entry = call_helper(
        b,
        ctx.cc,
        ctx.helpers.jit_call_method_window,
        &[ectx, niv, csv, w, total, out],
    );
    super::super::call::run_entered_or(b, ctx, entry, out);
    Ok(Some(None))
}

pub(super) fn spill(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    live: &[u32],
) -> Result<(), String> {
    for &v in live {
        let Ok(x) = load_value(b, ctx, values, v) else {
            continue;
        };
        let boxed = match ctx.ssa.value_ty(v) {
            varn_types::register_meta::SlotKind::Int => super::super::super::emit::box_int(b, x),
            varn_types::register_meta::SlotKind::Float => super::super::super::emit::box_f64(b, x),
            varn_types::register_meta::SlotKind::Bool => super::super::super::emit::box_bool(b, x),
            varn_types::register_meta::SlotKind::Str
            | varn_types::register_meta::SlotKind::Ref
            | varn_types::register_meta::SlotKind::Dynamic => x,
        };
        home_store(b, ctx, ctx.ssa.reg(v), boxed)?;
    }
    Ok(())
}
