use super::super::super::emit::call_helper_void;
use super::super::heap::{boxed_value, exec_ctx};
use super::super::store::Out;
use super::super::Ctx;
use super::common::{frame, native};
use cranelift_codegen::ir::{types, InstBuilder, Value};
use cranelift_frontend::FunctionBuilder;

pub(super) fn load_global(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    name: &str,
) -> Result<Option<Option<Out>>, String> {
    let ectx = exec_ctx(ctx);
    let h = ctx.helpers;
    let f = frame(ctx)?;
    let ni = super::super::props::str_idx(ctx, name)? as i64;
    let niv = b.ins().iconst(types::I64, ni);
    call_helper_void(b, ctx.cc, h.load_global_by_name, &[ectx, f.closure, niv]);
    Ok(Some(Some(Out::Boxed(native(b, ectx, h)))))
}

pub(super) fn store_global(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &mut [Option<Value>],
    name: &str,
    value: u32,
) -> Result<Option<Option<Out>>, String> {
    let ectx = exec_ctx(ctx);
    let h = ctx.helpers;
    let f = frame(ctx)?;
    let boxed = boxed_value(b, ctx, values, value)?;
    let (t, p) = b.ins().isplit(boxed);
    let niv = b
        .ins()
        .iconst(types::I64, super::super::props::str_idx(ctx, name)? as i64);
    call_helper_void(
        b,
        ctx.cc,
        h.store_global_by_name,
        &[ectx, f.closure, niv, t, p],
    );
    Ok(Some(None))
}

pub(super) fn load_module(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    source: &str,
    own_ip: u32,
    live: &[u32],
) -> Result<Option<Option<Out>>, String> {
    let ectx = exec_ctx(ctx);
    let h = ctx.helpers;
    let f = frame(ctx)?;
    super::suspend::spill(b, ctx, values, live)?;
    let idx = super::super::props::str_idx(ctx, source)? as i64;
    let siv = b.ins().iconst(types::I64, idx);
    let oiv = b.ins().iconst(types::I64, i64::from(own_ip));
    call_helper_void(b, ctx.cc, h.load_module, &[ectx, f.closure, siv, oiv]);
    Ok(Some(Some(Out::Boxed(native(b, ectx, h)))))
}

pub(super) fn module_slot(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &mut [Option<Value>],
    object: u32,
    slot: u16,
) -> Result<Option<Option<Out>>, String> {
    let ectx = exec_ctx(ctx);
    let h = ctx.helpers;
    let o = boxed_value(b, ctx, values, object)?;
    let (ot, op_) = b.ins().isplit(o);
    let siv = b.ins().iconst(types::I64, i64::from(slot));
    call_helper_void(b, ctx.cc, h.load_module_slot, &[ectx, ot, op_, siv]);
    Ok(Some(Some(Out::Boxed(native(b, ectx, h)))))
}

pub(super) fn store_module_slot(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &mut [Option<Value>],
    slot: u16,
    value: u32,
) -> Result<Option<Option<Out>>, String> {
    let ectx = exec_ctx(ctx);
    let h = ctx.helpers;
    let v = boxed_value(b, ctx, values, value)?;
    let (t, p) = b.ins().isplit(v);
    let siv = b.ins().iconst(types::I64, i64::from(slot));
    call_helper_void(b, ctx.cc, h.store_module_slot, &[ectx, siv, t, p]);
    Ok(Some(None))
}
