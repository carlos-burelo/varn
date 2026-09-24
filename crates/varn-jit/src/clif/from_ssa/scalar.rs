//! Native scalar ops of the SSA lowering.
//!
//! Operations whose semantics map 1:1 onto a Cranelift instruction: integer/
//! float arithmetic (with the mandatory `int` overflow guard), bitwise/shift,
//! comparisons, negation, casts, and the two call forms. Heap instructions —
//! constants, aggregates, indexing, fields, strings, type questions — are
//! routed to [`super::heapvalue`] first; this file owns only the register-file
//! scalar domain.

use cranelift_codegen::ir::{
    condcodes::{FloatCC, IntCC},
    types, InstBuilder, Value,
};
use cranelift_frontend::FunctionBuilder;
use varn_types::register_meta::SlotKind;
use varn_types::ssa::{SsaBinOp, SsaOp, SsaUnOp};

use super::{boxed, call, def_heap, heapvalue, is_heap, load_value, Ctx};

/// Emit one defining instruction; `Ok(None)` means the instruction defines no
/// CLIF value of its own.
///
/// `dest` is the defined value id; its static kind decides whether the result
/// lands in a CLIF register (scalar) or in its VM home (heap).
pub(super) fn emit_inst(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &mut [Option<Value>],
    op: &SsaOp,
    dest: Option<u32>,
) -> Result<Option<Value>, String> {
    if let Some(v) = heapvalue::emit(b, ctx, values, op, dest)? {
        return Ok(v);
    }

    let dest_ty = dest.map(|d| ctx.ssa.value_ty(d));
    let v = match op {
        SsaOp::ConstInt(n) => b.ins().iconst(types::I64, *n),
        SsaOp::ConstFloat(f) => b.ins().f64const(*f),
        SsaOp::ConstBool(x) => b.ins().iconst(types::I64, i64::from(*x)),
        SsaOp::Convert { operand, conv } => match (conv, ctx.ssa.value_ty(*operand)) {
            (varn_core::NumConv::IntToFloat, SlotKind::Int) => {
                let a = load_value(b, ctx, values, *operand)?;
                b.ins().fcvt_from_sint(types::F64, a)
            }
            _ => return Err(format!("from_ssa: convert {conv:?}")),
        },
        // Representation-neutral: every real conversion is a `Convert`.
        SsaOp::Cast { operand } => {
            let a = load_value(b, ctx, values, *operand)?;
            match (ctx.ssa.value_ty(*operand), dest_ty) {
                (SlotKind::Int, Some(SlotKind::Float)) => b.ins().fcvt_from_sint(types::F64, a),
                (SlotKind::Int, Some(SlotKind::Int))
                | (SlotKind::Float, Some(SlotKind::Float))
                | (SlotKind::Bool, Some(SlotKind::Bool)) => a,
                // A heap-to-heap cast is an alias too: land it in the dest home.
                (_, Some(k)) if is_heap(k) => {
                    let d = dest.ok_or("from_ssa: cast without dest")?;
                    def_heap(b, ctx, ctx.ssa.reg(d), a)?;
                    return Ok(Some(a));
                }
                _ => return Err("from_ssa: unsupported cast".into()),
            }
        }

        SsaOp::Binary { op, lhs, rhs } => {
            let a = load_value(b, ctx, values, *lhs)?;
            let c = load_value(b, ctx, values, *rhs)?;
            emit_bin(b, ctx, *op, a, c)?
        }
        SsaOp::Unary { op, operand } => {
            let a = load_value(b, ctx, values, *operand)?;
            emit_un(b, ctx, *op, a)?
        }
        SsaOp::SelfCall { args } => {
            // A frame-aware callee needs its own frame pushed (the raw ABI
            // prepends `stack, closure, base, exec_ctx`); this lowering does not
            // model that, so it declines and the bytecode lowering takes it.
            if ctx.frame.is_some() {
                return Err("from_ssa: frame-aware self-call".into());
            }
            let a: Vec<Value> = args
                .iter()
                .map(|v| load_value(b, ctx, values, *v))
                .collect::<Result<_, _>>()?;
            let call = b.ins().call(ctx.self_ref, &a);
            b.inst_results(call)[0]
        }
        SsaOp::Call {
            callee,
            callee_global,
            args,
        } => {
            let d = dest.ok_or("from_ssa: call without dest")?;
            call::emit_call(b, ctx, values, *callee, *callee_global, args, d)?
        }

        SsaOp::NarrowRangeCheck { .. } => return Err("from_ssa: range check".into()),

        // Handled by `heapvalue` above.
        SsaOp::ConstNull
        | SsaOp::ConstStr(_)
        | SsaOp::LoadGlobalIdx(_)
        | SsaOp::This
        | SsaOp::IsNull { .. }
        | SsaOp::IsArray { .. }
        | SsaOp::GetEnumTag { .. }
        | SsaOp::Typeof { .. }
        | SsaOp::ToString { .. }
        | SsaOp::ObjectKeys { .. }
        | SsaOp::BuildStr { .. }
        | SsaOp::BuildArray { .. }
        | SsaOp::BuildMap { .. }
        | SsaOp::BuildObject { .. }
        | SsaOp::GetIndex { .. }
        | SsaOp::GetProperty { .. }
        | SsaOp::SetProperty { .. }
        | SsaOp::ArrayLength { .. }
        | SsaOp::GetFixedField { .. }
        | SsaOp::SetIndex { .. }
        | SsaOp::ArrayPush { .. }
        | SsaOp::SetFixedField { .. }
        | SsaOp::MakeClass { .. }
        | SsaOp::DeclareField { .. }
        | SsaOp::DefineMethod { .. }
        | SsaOp::GetSuper { .. } => {
            unreachable!("heap ops are emitted by heapvalue")
        }
    };
    Ok(Some(v))
}

fn emit_bin(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    op: SsaBinOp,
    a: Value,
    c: Value,
) -> Result<Value, String> {
    use SsaBinOp::*;
    // Integer division/mod/power and float mod/power keep the VM's exact
    // semantics, faults included, through a runtime helper.
    if matches!(op, IntDiv | IntMod | IntPow | FloatMod | FloatPow) {
        let dest_float = matches!(op, FloatMod | FloatPow);
        return boxed::emit_bin(b, ctx, op, a, c, dest_float);
    }
    Ok(match op {
        IntAdd | IntSub | IntMul => checked_int(b, ctx, op, a, c),
        IntAnd => b.ins().band(a, c),
        IntOr => b.ins().bor(a, c),
        IntXor => b.ins().bxor(a, c),
        IntShl => {
            let sh = b.ins().band_imm(c, 0x3F);
            b.ins().ishl(a, sh)
        }
        IntShr => {
            let sh = b.ins().band_imm(c, 0x3F);
            b.ins().sshr(a, sh)
        }
        IntUshr => {
            let sh = b.ins().band_imm(c, 0x3F);
            b.ins().ushr(a, sh)
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

        StrConcat => return Err("from_ssa: concat is a heap op".into()),
        IntDiv | IntMod | IntPow | FloatMod | FloatPow => unreachable!("delegated to boxed"),
    })
}

fn checked_int(b: &mut FunctionBuilder, ctx: &Ctx<'_>, op: SsaBinOp, a: Value, c: Value) -> Value {
    use super::super::emit::guard_overflow;
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
        _ => {
            let (r, o) = b.ins().smul_overflow(a, c);
            (r, o, helpers.mul)
        }
    };
    // The `exec_ctx` argument is only read on the cold raise path, which
    // recovers the live context through the getter instead; a leaf body has no
    // real `exec_ctx` to pass, and this placeholder never reaches an
    // instruction.
    let dummy = b.ins().iconst(types::I64, 0);
    guard_overflow(
        b,
        ctx.cc,
        dummy,
        Some(helpers.current_exec_ctx),
        helper,
        r,
        ovf,
        a,
        c,
    )
}

fn emit_un(b: &mut FunctionBuilder, ctx: &Ctx<'_>, op: SsaUnOp, a: Value) -> Result<Value, String> {
    use super::super::emit::{box_int, call_helper, call_helper_void};
    Ok(match op {
        SsaUnOp::NegInt => {
            let cc = ctx.cc;
            let helpers = ctx.helpers;
            let neg = b.ins().ineg(a);
            let fits = b.ins().icmp_imm(IntCC::NotEqual, a, i64::MIN);
            let raise = b.create_block();
            let cont = b.create_block();
            b.ins().brif(fits, cont, &[], raise, &[]);
            b.switch_to_block(raise);
            let live = call_helper(b, cc, helpers.current_exec_ctx, &[]);
            let boxed = box_int(b, a);
            let (tag, payload) = b.ins().isplit(boxed);
            call_helper_void(b, cc, helpers.negate, &[live, tag, payload]);
            b.ins().jump(cont, &[]);
            b.switch_to_block(cont);
            neg
        }
        SsaUnOp::NegFloat => b.ins().fneg(a),
        SsaUnOp::Not => b.ins().bxor_imm(a, 1),
        SsaUnOp::BitNotInt => b.ins().bxor_imm(a, -1),
    })
}

fn bool_i64(b: &mut FunctionBuilder, cc: IntCC, a: Value, c: Value) -> Value {
    let bit = b.ins().icmp(cc, a, c);
    b.ins().uextend(types::I64, bit)
}

fn bool_f64(b: &mut FunctionBuilder, cc: FloatCC, a: Value, c: Value) -> Value {
    let bit = b.ins().fcmp(cc, a, c);
    b.ins().uextend(types::I64, bit)
}
