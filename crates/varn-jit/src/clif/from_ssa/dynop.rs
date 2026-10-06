






use cranelift_codegen::ir::{types, InstBuilder, Value};
use cranelift_frontend::FunctionBuilder;
use varn_types::register_meta::SlotKind;
use varn_types::ssa::{DynBinOp, DynUnOp};

use super::super::emit::{box_bool, box_int, call_helper, call_helper_void};
use super::super::generic::{boxed_binop, boxed_compare};
use super::heap::boxed_parts;
use super::{Ctx, Out};


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
    let ectx = ctx.exec_ctx;
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
    let ectx = ctx.exec_ctx;
    let result_off = h.jit_native_result_offset as i32;
    Ok(match op {
        DynUnOp::Neg => {
            call_helper_void(b, ctx.cc, h.negate, &[ectx, tag, payload]);
            Out::Boxed(b.ins().load(
                types::I128,
                cranelift_codegen::ir::MemFlagsData::trusted(),
                ectx,
                result_off,
            ))
        }
        DynUnOp::Not => {
            let cond = call_helper(b, ctx.cc, h.logical_not, &[ectx, tag, payload]);
            bool_out(b, cond, dest)
        }
        
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
