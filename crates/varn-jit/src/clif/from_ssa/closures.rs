//! Closures for the SSA lowering: creating one, the creating function's
//! captured variables, and a closure body's own upvalues.
//!
//! A captured variable is not an SSA value. It lives in its frame register
//! (`SsaProto::captured`) for its whole life, the register an open upvalue
//! points at, so every read and write goes to that home and there is no
//! CLIF copy to keep in step. Creating a closure and a closure body's
//! upvalue accesses run the runtime's one implementation of each.

use cranelift_codegen::ir::{types, InstBuilder, MemFlags, StackSlotData, StackSlotKind, Value};
use cranelift_frontend::FunctionBuilder;
use varn_types::ssa::{SsaUpvalue, UPVALUE_LOCAL};

use super::super::emit::call_helper_void;
use super::{def_heap, heap, use_heap, Ctx, FrameIo};

fn frame<'a>(ctx: &'a Ctx<'_>) -> Result<&'a FrameIo<'a>, String> {
    ctx.frame
        .as_ref()
        .ok_or_else(|| "from_ssa: closure op without a frame".into())
}

fn captured_reg(ctx: &Ctx<'_>, var: u32) -> Result<u32, String> {
    ctx.ssa
        .captured_reg(var)
        .ok_or_else(|| format!("from_ssa: captured variable {var} has no register"))
}

/// The boxed value a helper left in `jit_native_result`.
fn native_result(b: &mut FunctionBuilder, ctx: &Ctx<'_>, exec_ctx: Value) -> Value {
    b.ins().load(
        types::I128,
        MemFlags::trusted(),
        exec_ctx,
        ctx.helpers.jit_native_result_offset as i32,
    )
}

/// The closure of function constant `proto`, capturing `upvalues`: their
/// sources as a window of words (`UpvalueSrc::from_word`), handed to
/// `jit_make_closure_window` with this activation. The boxed closure.
pub(super) fn emit_make_closure(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    proto: u32,
    upvalues: &[SsaUpvalue],
) -> Result<Value, String> {
    let frame = frame(ctx)?;
    let words = upvalues
        .iter()
        .map(|src| {
            Ok(match src {
                SsaUpvalue::Captured(var) => UPVALUE_LOCAL | u64::from(captured_reg(ctx, *var)?),
                SsaUpvalue::Inherited(idx) => u64::from(*idx),
            })
        })
        .collect::<Result<Vec<u64>, String>>()?;
    let descs = if words.is_empty() {
        b.ins().iconst(types::I64, 0)
    } else {
        let slot = b.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            (words.len() * 8) as u32,
            3,
        ));
        let addr = b.ins().stack_addr(types::I64, slot, 0);
        for (i, w) in words.iter().enumerate() {
            let w = b.ins().iconst(types::I64, *w as i64);
            b.ins().store(MemFlags::trusted(), w, addr, (i * 8) as i32);
        }
        addr
    };
    let proto_v = b.ins().iconst(types::I64, i64::from(proto));
    let count = b.ins().iconst(types::I64, words.len() as i64);
    call_helper_void(
        b,
        ctx.cc,
        ctx.helpers.make_closure_window,
        &[
            frame.exec_ctx,
            frame.closure,
            frame.base,
            proto_v,
            descs,
            count,
        ],
    );
    Ok(native_result(b, ctx, frame.exec_ctx))
}

/// Captured variable `var`, boxed, from its home.
pub(super) fn emit_load_captured(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    var: u32,
) -> Result<Value, String> {
    use_heap(b, ctx, captured_reg(ctx, var)?)
}

/// Write `value` to captured variable `var`'s home, in the home's class.
pub(super) fn emit_store_captured(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    var: u32,
    value: u32,
) -> Result<(), String> {
    let boxed = heap::boxed_value(b, ctx, values, value)?;
    def_heap(b, ctx, captured_reg(ctx, var)?, boxed)
}

/// This closure's upvalue `index`, boxed.
pub(super) fn emit_load_upvalue(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    index: u32,
) -> Result<Value, String> {
    let frame = frame(ctx)?;
    let idx = b.ins().iconst(types::I64, i64::from(index));
    call_helper_void(
        b,
        ctx.cc,
        ctx.helpers.load_upvalue,
        &[frame.exec_ctx, frame.closure, idx],
    );
    Ok(native_result(b, ctx, frame.exec_ctx))
}

/// Write `value` to this closure's upvalue `index`.
pub(super) fn emit_store_upvalue(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    index: u32,
    value: u32,
) -> Result<(), String> {
    let frame = frame(ctx)?;
    let (tag, payload) = heap::boxed_parts(b, ctx, values, value)?;
    let idx = b.ins().iconst(types::I64, i64::from(index));
    call_helper_void(
        b,
        ctx.cc,
        ctx.helpers.store_upvalue,
        &[frame.exec_ctx, frame.closure, idx, tag, payload],
    );
    Ok(())
}

/// Close the open upvalues from the lowest register of captured `vars` up.
pub(super) fn emit_close_upvalues(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    vars: &[u32],
) -> Result<(), String> {
    let frame = frame(ctx)?;
    let lowest = vars
        .iter()
        .map(|v| captured_reg(ctx, *v))
        .collect::<Result<Vec<u32>, String>>()?
        .into_iter()
        .min()
        .ok_or("from_ssa: closing no upvalues")?;
    let lowest = b.ins().iconst(types::I64, i64::from(lowest));
    call_helper_void(
        b,
        ctx.cc,
        ctx.helpers.close_upvalue,
        &[frame.exec_ctx, lowest],
    );
    Ok(())
}
