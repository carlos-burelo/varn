use super::super::super::emit::call_helper_void;
use super::super::heap::{boxed_value, exec_ctx};
use super::super::store::Out;
use super::super::Ctx;
use super::extra_shared::{frame, native, stage};
use cranelift_codegen::ir::{types, InstBuilder, Value};
use cranelift_frontend::FunctionBuilder;
use varn_types::ssa::{SsaObjectSpreadPart, SsaSpread};

pub(super) fn tuple(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &mut [Option<Value>],
    elements: &[u32],
) -> Result<Option<Option<Out>>, String> {
    let h = ctx.helpers;
    let vals: Vec<Value> = elements
        .iter()
        .map(|v| boxed_value(b, ctx, values, *v))
        .collect::<Result<_, _>>()?;
    let r = window_result(b, ctx, h.build_array_window, &vals);
    Ok(Some(Some(Out::Boxed(r))))
}

pub(super) fn array_spread(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    elements: &[SsaSpread],
) -> Result<Option<Option<Out>>, String> {
    frame(ctx)?;
    let r = emit_array_spread(b, ctx, values, elements)?;
    Ok(Some(Some(Out::Boxed(r))))
}

pub(super) fn object_spread(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    parts: &[SsaObjectSpreadPart],
    cs_base: u16,
) -> Result<Option<Option<Out>>, String> {
    frame(ctx)?;
    let r = emit_object_spread(b, ctx, values, parts, cs_base)?;
    Ok(Some(Some(Out::Boxed(r))))
}

pub(super) fn object_merge(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &mut [Option<Value>],
    target: u32,
    source: u32,
) -> Result<Option<Option<Out>>, String> {
    let ectx = exec_ctx(ctx);
    let h = ctx.helpers;
    let a = boxed_value(b, ctx, values, target)?;
    let c = boxed_value(b, ctx, values, source)?;
    let (at, ap) = b.ins().isplit(a);
    let (ct, cp) = b.ins().isplit(c);
    call_helper_void(b, ctx.cc, h.object_merge, &[ectx, at, ap, ct, cp]);
    Ok(Some(None))
}

pub(super) fn object_rest(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &mut [Option<Value>],
    object: u32,
    skip_keys: &[Box<str>],
) -> Result<Option<Option<Out>>, String> {
    let ectx = exec_ctx(ctx);
    let h = ctx.helpers;
    let o = boxed_value(b, ctx, values, object)?;
    let (ot, op_) = b.ins().isplit(o);
    let mut keys = Vec::with_capacity(skip_keys.len());
    for k in skip_keys {
        keys.push(super::super::pool::literal(
            b,
            ctx,
            "string",
            |l| matches!(l, varn_types::Literal::Str(t) if t.as_ref() == k.as_ref()),
        )?);
    }
    let (addr, n) = stage(b, ctx, &keys);
    call_helper_void(b, ctx.cc, h.object_rest_window, &[ectx, ot, op_, addr, n]);
    Ok(Some(Some(Out::Boxed(native(b, ectx, h)))))
}

fn window_result(b: &mut FunctionBuilder, ctx: &Ctx<'_>, helper: usize, vals: &[Value]) -> Value {
    let ectx = exec_ctx(ctx);
    let (addr, n) = stage(b, ctx, vals);
    call_helper_void(b, ctx.cc, helper, &[ectx, addr, n]);
    b.ins().load(
        types::I128,
        cranelift_codegen::ir::MemFlagsData::trusted(),
        ectx,
        ctx.helpers.jit_native_result_offset as i32,
    )
}

fn emit_array_spread(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    elements: &[SsaSpread],
) -> Result<Value, String> {
    let ectx = exec_ctx(ctx);
    let h = ctx.helpers;
    let slot = b.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
        cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
        16,
        4,
    ));
    let addr = b.ins().stack_addr(types::I64, slot, 0);
    let zero = b.ins().iconst(types::I64, 0);
    call_helper_void(b, ctx.cc, h.build_array_window, &[ectx, addr, zero]);
    let fresh = |b: &mut FunctionBuilder| {
        b.ins().load(
            types::I128,
            cranelift_codegen::ir::MemFlagsData::trusted(),
            ectx,
            h.jit_native_result_offset as i32,
        )
    };
    for s in elements {
        let arr = fresh(b);
        let (at, ap) = b.ins().isplit(arr);
        let v = boxed_value(b, ctx, values, s.value)?;
        let (vt, vp) = b.ins().isplit(v);
        if s.spread {
            call_helper_void(b, ctx.cc, h.array_extend, &[ectx, at, ap, vt, vp]);
        } else {
            call_helper_void(b, ctx.cc, h.array_push, &[ectx, at, ap, vt, vp]);
        }
    }
    Ok(fresh(b))
}

fn emit_object_spread(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    parts: &[SsaObjectSpreadPart],
    cs_base: u16,
) -> Result<Value, String> {
    let ectx = exec_ctx(ctx);
    let h = ctx.helpers;
    let frame = ctx
        .frame
        .as_ref()
        .ok_or("from_ssa: spread without a frame")?;
    call_helper_void(b, ctx.cc, h.build_empty_object, &[ectx]);
    let fresh = |b: &mut FunctionBuilder| {
        b.ins().load(
            types::I128,
            cranelift_codegen::ir::MemFlagsData::trusted(),
            ectx,
            h.jit_native_result_offset as i32,
        )
    };
    let mut keyed = 0u32;
    for p in parts {
        match &p.key {
            Some(k) => {
                let obj = fresh(b);
                let (ot, op_) = b.ins().isplit(obj);
                let v = boxed_value(b, ctx, values, p.value)?;
                let (vt, vp) = b.ins().isplit(v);
                let niv = b
                    .ins()
                    .iconst(types::I64, super::super::props::str_idx(ctx, k)? as i64);
                let csv = b
                    .ins()
                    .iconst(types::I64, i64::from(cs_base) + i64::from(keyed));
                let ipv = b.ins().iconst(types::I64, 0);
                call_helper_void(
                    b,
                    ctx.cc,
                    h.set_property_flat,
                    &[ectx, frame.closure, ot, op_, vt, vp, niv, csv, ipv],
                );

                super::super::store::drop_home_addrs(ctx);
                keyed += 1;
            }
            None => {
                let obj = fresh(b);
                let (ot, op_) = b.ins().isplit(obj);
                let v = boxed_value(b, ctx, values, p.value)?;
                let (vt, vp) = b.ins().isplit(v);
                call_helper_void(b, ctx.cc, h.object_merge, &[ectx, ot, op_, vt, vp]);
            }
        }
    }
    Ok(fresh(b))
}
