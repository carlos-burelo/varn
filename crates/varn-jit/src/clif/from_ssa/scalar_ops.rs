use super::{boxed, Ctx};
use cranelift_codegen::ir::{
    condcodes::{FloatCC, IntCC},
    types, InstBuilder, Value,
};
use cranelift_frontend::FunctionBuilder;
use varn_types::ssa::{SsaBinOp, SsaUnOp};

pub(super) fn emit_bin(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    op: SsaBinOp,
    a: Value,
    c: Value,

    in_range: bool,
    no_overflow: bool,
) -> Result<Value, String> {
    use SsaBinOp::*;
    if matches!(op, IntDiv | IntMod) {
        return Ok(super::int_div::emit(b, ctx, op, a, c));
    }
    if matches!(op, IntPow | FloatMod | FloatPow) {
        let dest_float = matches!(op, FloatMod | FloatPow);
        return boxed::emit_bin(b, ctx, op, a, c, dest_float);
    }
    Ok(match op {
        IntAdd if in_range || no_overflow => b.ins().iadd(a, c),
        IntSub if in_range || no_overflow => b.ins().isub(a, c),
        IntMul if no_overflow => b.ins().imul(a, c),
        IntAdd | IntSub | IntMul => checked_int(b, ctx, op, a, c),
        IntAnd => b.ins().band(a, c),
        IntOr => b.ins().bor(a, c),
        IntXor => b.ins().bxor(a, c),
        IntShl => {
            let r = b.ins().ishl(a, c);
            checked_shift(b, ctx, r, ctx.helpers.shl, a, c)
        }
        IntShr => {
            let r = b.ins().sshr(a, c);
            checked_shift(b, ctx, r, ctx.helpers.shr, a, c)
        }
        IntUshr => {
            let r = b.ins().ushr(a, c);
            checked_shift(b, ctx, r, ctx.helpers.ushr, a, c)
        }
        IntEq => bool_i64(b, IntCC::Equal, a, c),
        IntNe => bool_i64(b, IntCC::NotEqual, a, c),
        IntLt => bool_i64(b, IntCC::SignedLessThan, a, c),
        IntLe => bool_i64(b, IntCC::SignedLessThanOrEqual, a, c),
        IntGt => bool_i64(b, IntCC::SignedGreaterThan, a, c),
        IntGe => bool_i64(b, IntCC::SignedGreaterThanOrEqual, a, c),

        FloatAdd => b.ins().fadd(a, c),
        FloatSub => b.ins().fsub(a, c),
        FloatMul => b.ins().fmul(a, c),
        FloatDiv => b.ins().fdiv(a, c),
        FloatEq => bool_f64(b, FloatCC::Equal, a, c),
        FloatNe => bool_f64(b, FloatCC::NotEqual, a, c),
        FloatLt => bool_f64(b, FloatCC::LessThan, a, c),
        FloatLe => bool_f64(b, FloatCC::LessThanOrEqual, a, c),
        FloatGt => bool_f64(b, FloatCC::GreaterThan, a, c),
        FloatGe => bool_f64(b, FloatCC::GreaterThanOrEqual, a, c),

        Dyn(_) => return Err("from_ssa: a Dyn operator is lowered by dynop".into()),
        StrConcat | StrEq | StrNe => return Err("from_ssa: concat is a heap op".into()),
        IntDiv | IntMod | IntPow | FloatMod | FloatPow => unreachable!("lowered above"),
    })
}

pub(super) fn checked_int(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    op: SsaBinOp,
    a: Value,
    c: Value,
) -> Value {
    let helpers = ctx.helpers;
    let (r, ovf, helper) = match op {
        SsaBinOp::IntAdd => {
            let (r, o) = b.ins().sadd_overflow(a, c);
            (r, o, helpers.add)
        }
        SsaBinOp::IntSub => {
            let (r, o) = b.ins().ssub_overflow(a, c);
            (r, o, helpers.sub)
        }
        SsaBinOp::IntMul
        | SsaBinOp::IntDiv
        | SsaBinOp::IntMod
        | SsaBinOp::IntPow
        | SsaBinOp::IntEq
        | SsaBinOp::IntNe
        | SsaBinOp::IntLt
        | SsaBinOp::IntLe
        | SsaBinOp::IntGt
        | SsaBinOp::IntGe
        | SsaBinOp::IntAnd
        | SsaBinOp::IntOr
        | SsaBinOp::IntXor
        | SsaBinOp::IntShl
        | SsaBinOp::IntShr
        | SsaBinOp::IntUshr
        | SsaBinOp::FloatAdd
        | SsaBinOp::FloatSub
        | SsaBinOp::FloatMul
        | SsaBinOp::FloatDiv
        | SsaBinOp::FloatMod
        | SsaBinOp::FloatPow
        | SsaBinOp::FloatEq
        | SsaBinOp::FloatNe
        | SsaBinOp::FloatLt
        | SsaBinOp::FloatLe
        | SsaBinOp::FloatGt
        | SsaBinOp::FloatGe
        | SsaBinOp::StrConcat
        | SsaBinOp::StrEq
        | SsaBinOp::StrNe
        | SsaBinOp::Dyn(_) => {
            let (r, o) = b.ins().smul_overflow(a, c);
            (r, o, helpers.mul)
        }
    };

    guarded(b, ctx, helper, r, ovf, a, c)
}

fn checked_shift(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    r: Value,
    helper: usize,
    a: Value,
    c: Value,
) -> Value {
    let out_of_range = b
        .ins()
        .icmp_imm_u(IntCC::UnsignedGreaterThan, c, varn_core::SHIFT_MAX);
    guarded(b, ctx, helper, r, out_of_range, a, c)
}

fn guarded(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    helper: usize,
    r: Value,
    fault: Value,
    a: Value,
    c: Value,
) -> Value {
    use super::super::emit::guard_overflow;
    let helpers = ctx.helpers;
    let err_off = helpers.jit_error_flag_offset as i32;
    let res_off = helpers.jit_native_result_offset as i32;
    let (landing, err_ret) = ctx.exits(b);
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

pub(super) fn emit_un(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    op: SsaUnOp,
    a: Value,
) -> Result<Value, String> {
    use super::super::emit::{box_int, call_helper_void};
    Ok(match op {
        SsaUnOp::NegInt => {
            let cc = ctx.cc;
            let helpers = ctx.helpers;
            let err_off = helpers.jit_error_flag_offset as i32;
            let res_off = helpers.jit_native_result_offset as i32;
            let (landing, err_ret) = ctx.exits(b);
            let neg = b.ins().ineg(a);
            let fits = b.ins().icmp_imm_u(IntCC::NotEqual, a, i64::MIN);
            let raise = b.create_block();
            let cont = b.create_block();
            b.ins().brif(fits, cont, &[], raise, &[]);
            b.switch_to_block(raise);
            let boxed = box_int(b, a);
            let (tag, payload) = b.ins().isplit(boxed);
            let zero = b.ins().iconst(types::I64, 0);
            b.ins().store(
                cranelift_codegen::ir::MemFlagsData::trusted(),
                zero,
                ctx.exec_ctx,
                err_off,
            );
            call_helper_void(b, cc, helpers.negate, &[ctx.exec_ctx, tag, payload]);
            let flag = b.ins().load(
                types::I64,
                cranelift_codegen::ir::MemFlagsData::trusted(),
                ctx.exec_ctx,
                err_off,
            );
            match (landing, err_ret) {
                (Some(t), _) => {
                    let err_blk = b.create_block();
                    b.ins().brif(flag, err_blk, &[], cont, &[]);
                    b.switch_to_block(err_blk);
                    let eboxed = b.ins().load(
                        types::I128,
                        cranelift_codegen::ir::MemFlagsData::trusted(),
                        ctx.exec_ctx,
                        res_off,
                    );
                    b.declare_value_needs_stack_map(eboxed);
                    b.ins().store(
                        cranelift_codegen::ir::MemFlagsData::trusted(),
                        zero,
                        ctx.exec_ctx,
                        err_off,
                    );
                    b.ins().jump(t, &[eboxed.into()]);
                }
                (None, Some(e)) => {
                    b.ins().brif(flag, e, &[], cont, &[]);
                }
                (None, None) => {
                    b.ins().jump(cont, &[]);
                }
            }
            b.switch_to_block(cont);
            neg
        }
        SsaUnOp::NegFloat => b.ins().fneg(a),
        SsaUnOp::Not => b.ins().bxor_imm_u(a, 1),
        SsaUnOp::BitNotInt => b.ins().bxor_imm_u(a, -1),
        SsaUnOp::Dyn(_) => return Err("from_ssa: a Dyn operator is lowered by dynop".into()),
    })
}

pub(super) fn bool_i64(b: &mut FunctionBuilder, cc: IntCC, a: Value, c: Value) -> Value {
    let bit = b.ins().icmp(cc, a, c);
    b.ins().uextend(types::I64, bit)
}

pub(super) fn bool_f64(b: &mut FunctionBuilder, cc: FloatCC, a: Value, c: Value) -> Value {
    let bit = b.ins().fcmp(cc, a, c);
    b.ins().uextend(types::I64, bit)
}
