












use cranelift_codegen::ir::{types, InstBuilder, TrapCode, Value};
use cranelift_frontend::FunctionBuilder;

use super::super::emit::call_helper_void;
use super::{heap, home_store, Ctx, FrameIo};

fn frame<'a>(ctx: &'a Ctx<'_>) -> Result<&'a FrameIo<'a>, String> {
    ctx.frame
        .as_ref()
        .ok_or_else(|| "from_ssa: try/throw without a frame".into())
}



pub(super) fn emit_try(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    catch_ip: u32,
    catch_value: u32,
    live: &[u32],
) -> Result<(), String> {
    let frame = frame(ctx)?;
    for &v in live {
        let boxed = heap::boxed_value(b, ctx, values, v)?;
        home_store(b, ctx, ctx.ssa.reg(v), boxed)?;
    }
    let ip = b.ins().iconst(types::I64, i64::from(catch_ip));
    let reg = b
        .ins()
        .iconst(types::I64, i64::from(ctx.ssa.reg(catch_value)));
    call_helper_void(b, ctx.cc, ctx.helpers.try_push, &[frame.exec_ctx, ip, reg]);
    Ok(())
}


pub(super) fn emit_pop_try(b: &mut FunctionBuilder, ctx: &Ctx<'_>) -> Result<(), String> {
    let frame = frame(ctx)?;
    call_helper_void(b, ctx.cc, ctx.helpers.try_pop, &[frame.exec_ctx]);
    Ok(())
}


pub(super) fn emit_throw(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    value: u32,
) -> Result<(), String> {
    let frame = frame(ctx)?;
    let (tag, payload) = heap::boxed_parts(b, ctx, values, value)?;
    call_helper_void(
        b,
        ctx.cc,
        ctx.helpers.throw,
        &[frame.exec_ctx, tag, payload],
    );
    b.ins().trap(TrapCode::user(1).expect("non-zero trap code"));
    Ok(())
}
