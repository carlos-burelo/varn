use super::super::super::emit::{call_helper, call_helper_void};
use super::super::heap::{boxed_value, exec_ctx};
use super::super::load_value;
use super::super::store::{home_load, home_store, Out};
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
    let dv = b.ins().iconst(types::I64, i64::from(ctx.ssa.reg(d)));
    let rv = b.ins().iconst(types::I64, i64::from(resume_ip));
    match ctx.fused_awaits.get(&operand) {
        Some((callee, args)) => {
            let callee_v = load_value(b, ctx, values, *callee)?;
            let window = super::super::call::boxed_window(b, ctx, values, callee_v, args)?;
            let total = b.ins().iconst(types::I64, (args.len() + 1) as i64);
            call_helper_void(b, ctx.cc, h.await_call, &[ectx, window, total, dv, rv]);
        }
        None => {
            let v = boxed_value(b, ctx, values, operand)?;
            let (t, p) = b.ins().isplit(v);
            call_helper_void(b, ctx.cc, h.await_helper, &[ectx, t, p, dv, rv]);
        }
    }
    super::super::store::drop_home_addrs(ctx);
    super::super::exceptions::check_error_branch(b, ctx)?;
    Ok(Some(Some(Out::Boxed(home_load(b, ctx, ctx.ssa.reg(d))?))))
}

pub(in crate::clif::from_ssa) fn fused_await_calls(
    ssa: &varn_types::ssa::SsaProto,
    try_live: &rustc_hash::FxHashSet<u32>,
) -> rustc_hash::FxHashMap<u32, (u32, Vec<u32>)> {
    let mut fused = rustc_hash::FxHashMap::default();
    for blk in &ssa.blocks {
        for pair in blk.insts.windows(2) {
            let (varn_types::ssa::SsaOp::Call { callee, args }, Some(d)) =
                (&pair[0].op, pair[0].dest)
            else {
                continue;
            };
            let varn_types::ssa::SsaOp::Await { operand, live, .. } = &pair[1].op else {
                continue;
            };
            if *operand == d
                && !live.contains(&d)
                && !try_live.contains(&d)
                && super::super::store::is_heap(ssa.value_ty(*callee))
            {
                fused.insert(d, (*callee, args.clone()));
            }
        }
    }
    fused
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
    Ok(Some(Some(Out::Boxed(home_load(b, ctx, ctx.ssa.reg(d))?))))
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
    super::super::call::run_entered_or(b, ctx, entry, out)?;
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
