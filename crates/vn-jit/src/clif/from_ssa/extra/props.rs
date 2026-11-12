use super::super::heap::{boxed_value, exec_ctx};
use super::super::store::Out;
use super::super::Ctx;
use super::extra_shared::native;
use cranelift_codegen::ir::{types, InstBuilder, Value};
use cranelift_frontend::FunctionBuilder;

pub(super) fn property_maybe(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &mut [Option<Value>],
    object: u32,
    name: &str,
) -> Result<Option<Option<Out>>, String> {
    let ectx = exec_ctx(ctx);
    let h = ctx.helpers;
    let o = boxed_value(b, ctx, values, object)?;
    let (ot, op_) = b.ins().isplit(o);
    let niv = b
        .ins()
        .iconst(types::I64, super::super::props::str_idx(ctx, name)? as i64);
    crate::clif::from_ssa::exceptions::call_checked(
        b,
        ctx,
        h.get_property_maybe,
        &[ectx, ot, op_, niv],
    )?;
    Ok(Some(Some(Out::Boxed(native(b, ectx, h)))))
}

pub(super) fn assert_not_null(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &mut [Option<Value>],
    operand: u32,
) -> Result<Option<Option<Out>>, String> {
    let ectx = exec_ctx(ctx);
    let h = ctx.helpers;
    let v = boxed_value(b, ctx, values, operand)?;
    let (t, p) = b.ins().isplit(v);
    crate::clif::from_ssa::exceptions::call_checked(b, ctx, h.assert_not_null, &[ectx, t, p])?;
    Ok(Some(None))
}

pub(super) fn bind_method(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &mut [Option<Value>],
    object: u32,
    name: &str,
) -> Result<Option<Option<Out>>, String> {
    let ectx = exec_ctx(ctx);
    let h = ctx.helpers;
    let o = boxed_value(b, ctx, values, object)?;
    let (ot, op_) = b.ins().isplit(o);
    let niv = b
        .ins()
        .iconst(types::I64, super::super::props::str_idx(ctx, name)? as i64);
    crate::clif::from_ssa::exceptions::call_checked(b, ctx, h.bind_method, &[ectx, ot, op_, niv])?;
    Ok(Some(Some(Out::Boxed(native(b, ectx, h)))))
}

pub(super) fn array_extend(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &mut [Option<Value>],
    array: u32,
    source: u32,
) -> Result<Option<Option<Out>>, String> {
    let ectx = exec_ctx(ctx);
    let h = ctx.helpers;
    let a = boxed_value(b, ctx, values, array)?;
    let s = boxed_value(b, ctx, values, source)?;
    let (at, ap) = b.ins().isplit(a);
    let (st, sp) = b.ins().isplit(s);
    crate::clif::from_ssa::exceptions::call_checked(
        b,
        ctx,
        h.array_extend,
        &[ectx, at, ap, st, sp],
    )?;
    Ok(Some(None))
}

pub(super) fn wrap_spread(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &mut [Option<Value>],
    operand: u32,
) -> Result<Option<Option<Out>>, String> {
    let ectx = exec_ctx(ctx);
    let h = ctx.helpers;
    let v = boxed_value(b, ctx, values, operand)?;
    let (t, p) = b.ins().isplit(v);
    crate::clif::from_ssa::exceptions::call_checked(b, ctx, h.wrap_spread, &[ectx, t, p])?;
    Ok(Some(Some(Out::Boxed(native(b, ectx, h)))))
}

pub(super) fn range(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &mut [Option<Value>],
    start: u32,
    end: u32,
    inclusive: bool,
) -> Result<Option<Option<Out>>, String> {
    let ectx = exec_ctx(ctx);
    let h = ctx.helpers;
    let a = boxed_value(b, ctx, values, start)?;
    let c = boxed_value(b, ctx, values, end)?;
    let (at, ap) = b.ins().isplit(a);
    let (ct, cp) = b.ins().isplit(c);
    let f = b.ins().iconst(types::I64, i64::from(inclusive));
    crate::clif::from_ssa::exceptions::call_checked(b, ctx, h.range, &[ectx, at, ap, ct, cp, f])?;
    Ok(Some(Some(Out::Boxed(native(b, ectx, h)))))
}
