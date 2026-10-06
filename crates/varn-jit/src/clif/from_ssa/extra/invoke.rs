use super::super::super::emit::call_helper_void;
use super::super::heap::{boxed_value, exec_ctx};
use super::super::load_value;
use super::super::store::Out;
use super::super::Ctx;
use super::common::{frame, native, stage};
use cranelift_codegen::ir::{types, InstBuilder, Value};
use cranelift_frontend::FunctionBuilder;

pub(super) fn iter_call(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &mut [Option<Value>],
    callee: u32,
    recv: u32,
) -> Result<Option<Option<Out>>, String> {
    frame(ctx)?;
    let c = load_value(b, ctx, values, callee)?;
    let (ct, cp) = b.ins().isplit(c);
    let w = super::super::call::boxed_window(b, ctx, values, c, &[recv])?;
    let r = super::super::call::emit_invoke(b, ctx, w, (ct, cp), 2);
    Ok(Some(Some(Out::Boxed(r))))
}

pub(super) fn super_call(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    args: &[u32],
) -> Result<Option<Option<Out>>, String> {
    frame(ctx)?;
    let ctor = super::super::classops::emit_get_super(b, ctx, "constructor")?;
    let r = emit_super_call(b, ctx, values, ctor, args)?;
    Ok(Some(Some(Out::Boxed(r))))
}

pub(super) fn super_method_call(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &mut [Option<Value>],
    name: &str,
    args: &[u32],
) -> Result<Option<Option<Out>>, String> {
    frame(ctx)?;
    let m = super::super::classops::emit_get_super(b, ctx, name)?;
    let mut vals = Vec::with_capacity(args.len());
    for a in args {
        vals.push(boxed_value(b, ctx, values, *a)?);
    }
    let w = stage_value(b, ctx, m, &vals);
    let (mt, mp) = b.ins().isplit(m);
    let r = super::super::call::emit_invoke(b, ctx, w, (mt, mp), vals.len() + 1);
    Ok(Some(Some(Out::Boxed(r))))
}

pub(super) fn extension_call(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &mut [Option<Value>],
    func: &str,
    slot: Option<u32>,
    recv: u32,
    args: &[u32],
) -> Result<Option<Option<Out>>, String> {
    let ectx = exec_ctx(ctx);
    let h = ctx.helpers;
    let f = frame(ctx)?;
    let callee = match slot {
        Some(s) => {
            super::super::globals::emit_load(b, ctx, s, super::super::globals::Region::Module)?
        }
        None => {
            let niv = b
                .ins()
                .iconst(types::I64, super::super::props::str_idx(ctx, func)? as i64);
            call_helper_void(b, ctx.cc, h.load_global_by_name, &[ectx, f.closure, niv]);
            native(b, ectx, h)
        }
    };
    let mut full = Vec::with_capacity(args.len() + 1);
    full.push(recv);
    full.extend_from_slice(args);
    let w = super::super::call::boxed_window(b, ctx, values, callee, &full)?;
    let (ct, cp) = b.ins().isplit(callee);
    let r = super::super::call::emit_invoke(b, ctx, w, (ct, cp), full.len() + 1);
    Ok(Some(Some(Out::Boxed(r))))
}

pub(super) fn call_spread(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &mut [Option<Value>],
    callee: u32,
    args: &[varn_types::ssa::SsaSpread],
) -> Result<Option<Option<Out>>, String> {
    let ectx = exec_ctx(ctx);
    let h = ctx.helpers;
    let c = load_value(b, ctx, values, callee)?;
    let (ct, cp) = b.ins().isplit(c);
    let mut vals = Vec::with_capacity(args.len());
    for s in args {
        let v = boxed_value(b, ctx, values, s.value)?;
        if s.spread {
            let (t, p) = b.ins().isplit(v);
            call_helper_void(b, ctx.cc, h.wrap_spread, &[ectx, t, p]);
            vals.push(native(b, ectx, h));
        } else {
            vals.push(v);
        }
    }
    let (addr, _) = stage(b, ctx, &vals);
    let argc = b.ins().iconst(types::I64, vals.len() as i64);
    call_helper_void(
        b,
        ctx.cc,
        h.jit_call_spread_window,
        &[ectx, ct, cp, addr, argc],
    );
    Ok(Some(Some(Out::Boxed(native(b, ectx, h)))))
}

fn emit_super_call(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    ctor: Value,
    args: &[u32],
) -> Result<Value, String> {
    let this = super::super::props::emit_this(ctx)?;
    let mut vals = Vec::with_capacity(args.len() + 1);
    vals.push(this);
    for a in args {
        vals.push(boxed_value(b, ctx, values, *a)?);
    }
    let addr = super::super::call::scratch_addr(b, ctx, vals.len().max(1));
    for (i, v) in vals.iter().enumerate() {
        b.ins().store(
            cranelift_codegen::ir::MemFlagsData::trusted(),
            *v,
            addr,
            (i * 16) as i32,
        );
    }
    let (ct, cp) = b.ins().isplit(ctor);
    Ok(super::super::call::emit_invoke(
        b,
        ctx,
        addr,
        (ct, cp),
        vals.len(),
    ))
}

fn stage_value(b: &mut FunctionBuilder, ctx: &Ctx<'_>, callee: Value, args: &[Value]) -> Value {
    let addr = super::super::call::scratch_addr(b, ctx, args.len() + 1);
    b.ins().store(
        cranelift_codegen::ir::MemFlagsData::trusted(),
        callee,
        addr,
        0,
    );
    for (i, v) in args.iter().enumerate() {
        b.ins().store(
            cranelift_codegen::ir::MemFlagsData::trusted(),
            *v,
            addr,
            ((i + 1) * 16) as i32,
        );
    }
    addr
}
