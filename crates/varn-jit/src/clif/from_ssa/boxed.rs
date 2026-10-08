use cranelift_codegen::ir::{types, InstBuilder, Value};
use cranelift_frontend::FunctionBuilder;
use varn_types::ssa::SsaBinOp;

use super::Ctx;

use super::super::emit::{box_f64, box_int, call_helper_void, unbox_f64_coerce, unbox_int};

pub(super) fn emit_bin(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    op: SsaBinOp,
    a: Value,
    c: Value,
    dest_float: bool,
) -> Result<Value, String> {
    use SsaBinOp::*;
    let helpers = ctx.helpers;
    let cc = ctx.cc;
    let (helper, operand_float) = match op {
        IntPow => (helpers.pow, false),
        FloatMod => (helpers.modulo, true),
        FloatPow => (helpers.pow, true),
        IntAdd | IntSub | IntMul | IntDiv | IntMod | IntEq | IntNe | IntLt | IntLe | IntGt | IntGe | IntAnd | IntOr | IntXor | IntShl | IntShr | IntUshr | FloatAdd | FloatSub | FloatMul | FloatDiv | FloatEq | FloatNe | FloatLt | FloatLe | FloatGt | FloatGe | StrConcat | Dyn(_) => return Err("from_ssa: not a boxed op".into()),
    };

    let (a_tag, a_payload) = box_native(b, a, operand_float);
    let (b_tag, b_payload) = box_native(b, c, operand_float);
    let live = ctx.exec_ctx;
    call_helper_void(b, cc, helper, &[live, a_tag, a_payload, b_tag, b_payload]);
    let boxed = b.ins().load(
        types::I128,
        cranelift_codegen::ir::MemFlagsData::trusted(),
        live,
        helpers.jit_native_result_offset as i32,
    );
    Ok(if dest_float {
        unbox_f64_coerce(b, boxed)
    } else {
        unbox_int(b, boxed)
    })
}

fn box_native(b: &mut FunctionBuilder, v: Value, float: bool) -> (Value, Value) {
    let boxed = if float { box_f64(b, v) } else { box_int(b, v) };
    b.ins().isplit(boxed)
}
