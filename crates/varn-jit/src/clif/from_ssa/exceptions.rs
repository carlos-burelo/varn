//! `try` regions and `throw` for the SSA lowering.
//!
//! A compiled body runs the normal path only. `Try` pushes the runtime's
//! handler (`jit_push_try`) naming the landing pad's bytecode offset; a throw
//! — a `Throw` terminator or any helper that fails — unwinds to that handler
//! and the interpreter resumes this same activation at the landing pad, so
//! the catch path never runs compiled. What the landing pad reads must
//! therefore be where the interpreter reads it: every value live into it is
//! written to its home when the region opens. Heap values already live
//! there; scalars are stored once, since an SSA value never changes, and the
//! register allocator keeps a value's register for as long as the landing pad
//! may read it.

use cranelift_codegen::ir::{types, InstBuilder, TrapCode, Value};
use cranelift_frontend::FunctionBuilder;

use super::super::emit::call_helper_void;
use super::{def_heap, heap, is_heap, Ctx, FrameIo};

fn frame<'a>(ctx: &'a Ctx<'_>) -> Result<&'a FrameIo<'a>, String> {
    ctx.frame
        .as_ref()
        .ok_or_else(|| "from_ssa: try/throw without a frame".into())
}

/// Open a `try` region: the landing pad's live scalars to their homes, then
/// the handler, whose thrown value lands in `catch_value`'s register.
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
        if !is_heap(ctx.ssa.value_ty(v)) {
            let boxed = heap::boxed_value(b, ctx, values, v)?;
            def_heap(b, ctx, ctx.ssa.reg(v), boxed)?;
        }
    }
    let ip = b.ins().iconst(types::I64, i64::from(catch_ip));
    let reg = b
        .ins()
        .iconst(types::I64, i64::from(ctx.ssa.reg(catch_value)));
    call_helper_void(b, ctx.cc, ctx.helpers.try_push, &[frame.exec_ctx, ip, reg]);
    Ok(())
}

/// Close the innermost `try` region.
pub(super) fn emit_pop_try(b: &mut FunctionBuilder, ctx: &Ctx<'_>) -> Result<(), String> {
    let frame = frame(ctx)?;
    call_helper_void(b, ctx.cc, ctx.helpers.try_pop, &[frame.exec_ctx]);
    Ok(())
}

/// `throw value`: the runtime's throw, which unwinds and does not return.
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
