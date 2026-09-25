//! Operators on boxed values (`SsaBinOp::Dyn`, `SsaUnOp::Dyn`): the
//! bytecode's generic opcodes, for operands no type proves native. They run
//! the same runtime helpers through the same lowering as the bytecode path
//! (`clif::generic::{boxed_binop, boxed_compare}`), so an operator means the
//! same thing compiled either way. The live `ExecCtx` comes from the frame, or
//! from `current_exec_ctx` in a leaf body, which has no `exec_ctx` parameter.

use cranelift_codegen::ir::{types, InstBuilder, MemFlags, Value};
use cranelift_frontend::FunctionBuilder;
use varn_types::register_meta::SlotKind;
use varn_types::ssa::{DynBinOp, DynUnOp};

use super::super::emit::{box_bool, box_int, call_helper, call_helper_void};
use super::super::generic::{boxed_binop, boxed_compare};
use super::heap::boxed_parts;
use super::{Ctx, Out};

/// The live `ExecCtx`.
fn exec_ctx(b: &mut FunctionBuilder, ctx: &Ctx<'_>) -> Value {
    match &ctx.frame {
        Some(frame) => frame.exec_ctx,
        None => call_helper(b, ctx.cc, ctx.helpers.current_exec_ctx, &[]),
    }
}

/// A comparison's `0`/`1` as the destination holds it.
fn bool_out(b: &mut FunctionBuilder, cond: Value, dest: Option<SlotKind>) -> Out {
    match dest {
        Some(SlotKind::Bool) => Out::Native(cond),
        _ => Out::Boxed(box_bool(b, cond)),
    }
}

pub(super) fn emit_bin(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    op: DynBinOp,
    lhs: u32,
    rhs: u32,
    dest: Option<SlotKind>,
) -> Result<Out, String> {
    let h = ctx.helpers;
    let a = boxed_parts(b, ctx, values, lhs)?;
    let c = boxed_parts(b, ctx, values, rhs)?;
    let ectx = exec_ctx(b, ctx);
    let result_off = h.jit_native_result_offset as i32;
    let arith = |b: &mut FunctionBuilder, helper| {
        Out::Boxed(boxed_binop(b, ctx.cc, helper, ectx, result_off, a, c))
    };
    let compare = |b: &mut FunctionBuilder, helper, equality| {
        let cond = boxed_compare(b, ctx.cc, helper, ectx, equality, a, c);
        bool_out(b, cond, dest)
    };
    Ok(match op {
        DynBinOp::Add => arith(b, h.add),
        DynBinOp::Sub => arith(b, h.sub),
        DynBinOp::Mul => arith(b, h.mul),
        DynBinOp::Div => arith(b, h.div),
        DynBinOp::Mod => arith(b, h.modulo),
        DynBinOp::Pow => arith(b, h.pow),
        DynBinOp::BitAnd => arith(b, h.bit_and),
        DynBinOp::BitOr => arith(b, h.bit_or),
        DynBinOp::BitXor => arith(b, h.bit_xor),
        DynBinOp::Shl => arith(b, h.shl),
        DynBinOp::Shr => arith(b, h.shr),
        DynBinOp::Ushr => arith(b, h.ushr),
        DynBinOp::Eq => compare(b, h.eq, Some(true)),
        DynBinOp::Ne => compare(b, h.neq, Some(false)),
        DynBinOp::Lt => compare(b, h.lt, None),
        DynBinOp::Le => compare(b, h.lte, None),
        DynBinOp::Gt => compare(b, h.gt, None),
        DynBinOp::Ge => compare(b, h.gte, None),
        DynBinOp::Instanceof => compare(b, h.instanceof, None),
        DynBinOp::In => compare(b, h.op_in, None),
    })
}

pub(super) fn emit_un(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &[Option<Value>],
    op: DynUnOp,
    operand: u32,
    dest: Option<SlotKind>,
) -> Result<Out, String> {
    let h = ctx.helpers;
    let (tag, payload) = boxed_parts(b, ctx, values, operand)?;
    let ectx = exec_ctx(b, ctx);
    let result_off = h.jit_native_result_offset as i32;
    Ok(match op {
        DynUnOp::Neg => {
            call_helper_void(b, ctx.cc, h.negate, &[ectx, tag, payload]);
            Out::Boxed(
                b.ins()
                    .load(types::I128, MemFlags::trusted(), ectx, result_off),
            )
        }
        DynUnOp::Not => {
            let cond = call_helper(b, ctx.cc, h.logical_not, &[ectx, tag, payload]);
            bool_out(b, cond, dest)
        }
        // `~x` is `x ^ -1`, as the bytecode emits it.
        DynUnOp::BitNot => {
            let minus_one = b.ins().iconst(types::I64, -1);
            let boxed = box_int(b, minus_one);
            let m1 = b.ins().isplit(boxed);
            Out::Boxed(boxed_binop(
                b,
                ctx.cc,
                h.bit_xor,
                ectx,
                result_off,
                (tag, payload),
                m1,
            ))
        }
    })
}
