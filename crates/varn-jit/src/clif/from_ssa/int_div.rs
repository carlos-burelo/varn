use cranelift_codegen::ir::{condcodes::IntCC, InstBuilder, Value};
use cranelift_frontend::FunctionBuilder;
use varn_types::ssa::SsaBinOp;

use super::super::emit::guard_overflow;
use super::Ctx;

pub(super) fn emit(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    op: SsaBinOp,
    a: Value,
    c: Value,
) -> Value {
    let zero = b.ins().icmp_imm_u(IntCC::Equal, c, 0);
    let minus_one = b.ins().icmp_imm_u(IntCC::Equal, c, -1);
    let one = b.ins().iconst(cranelift_codegen::ir::types::I64, 1);
    let (r, fault, helper) = if op == SsaBinOp::IntDiv {
        let min = b.ins().icmp_imm_u(IntCC::Equal, a, i64::MIN);
        let overflow = b.ins().band(min, minus_one);
        let fault = b.ins().bor(zero, overflow);
        let divisor = b.ins().select(fault, one, c);
        (b.ins().sdiv(a, divisor), fault, ctx.helpers.div)
    } else {
        let trapping = b.ins().bor(zero, minus_one);
        let divisor = b.ins().select(trapping, one, c);
        (b.ins().srem(a, divisor), zero, ctx.helpers.modulo)
    };
    let err_off = ctx.helpers.jit_error_flag_offset as i32;
    let res_off = ctx.helpers.jit_native_result_offset as i32;
    let (landing, err_ret) = {
        let st = ctx.pure_try.borrow();
        let landing = st.stack.last().and_then(|h| st.clif_map.get(h).copied());
        (landing, st.err_ret)
    };
    guard_overflow(
        b,
        ctx.cc,
        ctx.exec_ctx,
        helper,
        r,
        fault,
        a,
        c,
        err_off,
        res_off,
        landing,
        err_ret,
    )
}
