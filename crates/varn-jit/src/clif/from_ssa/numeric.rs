//! `std:math` intrinsics and numeric conversions (`as`) for the SSA lowering.
//!
//! Each runs the runtime's one implementation through a helper —
//! `intrinsics::dispatch` over a boxed window, `convert::convert` — except
//! where a single instruction is provably the same function:
//!
//! * `abs`/`sqrt`/`floor`/`ceil` on a **float** argument are
//!   `fabs`/`sqrt`/`floor`/`ceil` (the latter two only with SSE4.1, which
//!   Cranelift needs for `roundsd`). An int argument keeps the helper, which
//!   re-boxes an integral result as `int`; `round`/`min`/`max` disagree with
//!   the ISA (ties, NaN) and the rest have no instruction. The same proof
//!   the bytecode lowering's `floats::emit_math_intrinsic_native` documents.
//! * `int` → `float` is `fcvt_from_sint`, what `convert`'s `to_float` does.

use cranelift_codegen::ir::{types, InstBuilder, MemFlags, Value};
use cranelift_frontend::FunctionBuilder;
use varn_core::intrinsic_ops::math::MathOp;
use varn_core::NumConv;
use varn_types::register_meta::SlotKind;
use varn_types::ssa::SsaProto;

use super::super::emit::{box_f64, call_helper_void};
use super::{call, heap, load_value, Ctx, Out};

fn exec_ctx(ctx: &Ctx<'_>) -> Result<Value, String> {
    heap::exec_ctx(ctx)
}

fn native_result(b: &mut FunctionBuilder, ctx: &Ctx<'_>, ectx: Value) -> Value {
    b.ins().load(
        types::I128,
        MemFlags::trusted(),
        ectx,
        ctx.helpers.jit_native_result_offset as i32,
    )
}

/// The single instruction a unary math op on a float is, if it is one.
fn float_instruction(b: &mut FunctionBuilder, ctx: &Ctx<'_>, wire: u8, x: Value) -> Option<Value> {
    Some(match wire {
        w if w == MathOp::Abs as u8 => b.ins().fabs(x),
        w if w == MathOp::Sqrt as u8 => b.ins().sqrt(x),
        w if w == MathOp::Floor as u8 && ctx.has_round => b.ins().floor(x),
        w if w == MathOp::Ceil as u8 && ctx.has_round => b.ins().ceil(x),
        _ => return None,
    })
}

/// `wire(object, args...)`.
pub(super) fn emit_intrinsic(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    object: u32,
    args: &[u32],
    wire: u8,
    dest: Option<u32>,
) -> Result<Out, String> {
    if let [arg] = args {
        if ctx.ssa.value_ty(*arg) == SlotKind::Float {
            let x = load_value(b, ctx, values, *arg)?;
            if let Some(r) = float_instruction(b, ctx, wire, x) {
                let float_dest = dest.is_some_and(|d| ctx.ssa.value_ty(d) == SlotKind::Float);
                return Ok(if float_dest {
                    Out::Native(r)
                } else {
                    Out::Boxed(box_f64(b, r))
                });
            }
        }
    }
    let ectx = exec_ctx(ctx)?;
    let receiver = heap::boxed_value(b, ctx, values, object)?;
    let window = call::boxed_window(b, ctx, values, receiver, args)?;
    let wire_v = b.ins().iconst(types::I64, i64::from(wire));
    let count = b.ins().iconst(types::I64, (args.len() + 1) as i64);
    call_helper_void(
        b,
        ctx.cc,
        ctx.helpers.intrinsic_window,
        &[ectx, wire_v, window, count],
    );
    Ok(Out::Boxed(native_result(b, ctx, ectx)))
}

/// Whether `operand as T` is the one instruction rather than the helper.
pub(super) fn is_inline_convert(ssa: &SsaProto, operand: u32, conv: NumConv) -> bool {
    conv == NumConv::IntToFloat && ssa.value_ty(operand) == SlotKind::Int
}

/// `operand as T` for numeric conversion `conv`.
pub(super) fn emit_convert(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    operand: u32,
    conv: NumConv,
) -> Result<Out, String> {
    if is_inline_convert(ctx.ssa, operand, conv) {
        let a = load_value(b, ctx, values, operand)?;
        return Ok(Out::Native(b.ins().fcvt_from_sint(types::F64, a)));
    }
    let ectx = exec_ctx(ctx)?;
    let (tag, payload) = heap::boxed_parts(b, ctx, values, operand)?;
    let conv_v = b.ins().iconst(types::I64, conv as i64);
    call_helper_void(
        b,
        ctx.cc,
        ctx.helpers.convert,
        &[ectx, conv_v, tag, payload],
    );
    Ok(Out::Boxed(native_result(b, ctx, ectx)))
}
