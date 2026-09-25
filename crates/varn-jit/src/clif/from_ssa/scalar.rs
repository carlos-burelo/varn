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

use super::{
    boxed, call, closures, dynop, globals, heap, heapvalue, is_heap, load_value, Ctx, Out,
};

/// Emit one instruction; `Ok(None)` means it produces no result. The driver
/// lands a result where `dest`'s class says it lives ([`super::store::land`]).
pub(super) fn emit_inst(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &mut [Option<Value>],
    op: &SsaOp,
    dest: Option<u32>,
) -> Result<Option<Out>, String> {
    if let Some(out) = heapvalue::emit(b, ctx, values, op, dest)? {
        return Ok(out);
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
        // Representation-neutral: every real conversion is a `Convert`. What a
        // cast may change is the storage class — a scalar entering a heap
        // (`Dynamic`) value is boxed, a heap value entering a scalar one is
        // unboxed, heap to heap is the same `VmValue`.
        SsaOp::Cast { operand } => {
            let from = ctx.ssa.value_ty(*operand);
            let to = dest_ty.ok_or("from_ssa: cast without dest")?;
            let a = load_value(b, ctx, values, *operand)?;
            return Ok(Some(match (from, to) {
                (SlotKind::Int, SlotKind::Float) => {
                    Out::Native(b.ins().fcvt_from_sint(types::F64, a))
                }
                (SlotKind::Int, SlotKind::Int)
                | (SlotKind::Float, SlotKind::Float)
                | (SlotKind::Bool, SlotKind::Bool) => Out::Native(a),
                (SlotKind::Int | SlotKind::Float | SlotKind::Bool, to) if is_heap(to) => {
                    Out::Boxed(heap::boxed_value(b, ctx, values, *operand)?)
                }
                (from, _) if is_heap(from) => Out::Boxed(a),
                (from, to) => return Err(format!("from_ssa: cast {from:?} -> {to:?}")),
            }));
        }

        SsaOp::Binary {
            op: SsaBinOp::Dyn(op),
            lhs,
            rhs,
        } => {
            return Ok(Some(dynop::emit_bin(
                b, ctx, values, *op, *lhs, *rhs, dest_ty,
            )?))
        }
        SsaOp::Unary {
            op: SsaUnOp::Dyn(op),
            operand,
        } => {
            return Ok(Some(dynop::emit_un(
                b, ctx, values, *op, *operand, dest_ty,
            )?))
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
            // A frame-aware body cannot hand its own frame to the callee:
            // the recursion gets a fresh activation from the runtime.
            if ctx.frame.is_some() {
                return Ok(Some(Out::Boxed(call::emit_self_call_framed(
                    b, ctx, values, args,
                )?)));
            }
            let a: Vec<Value> = args
                .iter()
                .map(|v| load_value(b, ctx, values, *v))
                .collect::<Result<_, _>>()?;
            let call = b.ins().call(ctx.self_ref, &a);
            b.inst_results(call)[0]
        }
        SsaOp::MethodCall {
            recv,
            name,
            args,
            cs,
        } => {
            return Ok(Some(Out::Boxed(call::emit_method_call(
                b, ctx, values, *recv, name, args, *cs,
            )?)))
        }
        SsaOp::CallNativeOp {
            object,
            args,
            op_id,
        } => {
            return Ok(Some(Out::Boxed(call::emit_call_native_op(
                b, ctx, values, *object, args, *op_id,
            )?)))
        }
        SsaOp::Call {
            callee,
            callee_global,
            args,
        } => {
            return Ok(Some(call::emit_call(
                b,
                ctx,
                values,
                *callee,
                *callee_global,
                args,
                dest,
            )?))
        }

        SsaOp::MakeClosure { proto, upvalues } => {
            return Ok(Some(Out::Boxed(closures::emit_make_closure(
                b, ctx, *proto, upvalues,
            )?)))
        }
        SsaOp::LoadCaptured { var } => {
            return Ok(Some(Out::Boxed(closures::emit_load_captured(
                b, ctx, *var,
            )?)))
        }
        SsaOp::StoreCaptured { var, value } => {
            closures::emit_store_captured(b, ctx, values, *var, *value)?;
            return Ok(None);
        }
        SsaOp::LoadUpvalue(index) => {
            return Ok(Some(Out::Boxed(closures::emit_load_upvalue(
                b, ctx, *index,
            )?)))
        }
        SsaOp::StoreUpvalue { index, value } => {
            closures::emit_store_upvalue(b, ctx, values, *index, *value)?;
            return Ok(None);
        }
        SsaOp::StoreGlobalIdx { slot, value } => {
            let boxed = heap::boxed_value(b, ctx, values, *value)?;
            globals::emit_store(b, ctx, *slot, boxed)?;
            return Ok(None);
        }
        SsaOp::CloseUpvalues { vars } => {
            closures::emit_close_upvalues(b, ctx, vars)?;
            return Ok(None);
        }

        // Handled by `heapvalue` above.
        SsaOp::ConstNull
        | SsaOp::ConstStr(_)
        | SsaOp::LoadGlobalIdx(_)
        | SsaOp::LoadNativeGlobalIdx(_)
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
        | SsaOp::StrLength { .. }
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
    Ok(Some(Out::Native(v)))
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

        Dyn(_) => return Err("from_ssa: a Dyn operator is lowered by dynop".into()),
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
        SsaUnOp::Dyn(_) => return Err("from_ssa: a Dyn operator is lowered by dynop".into()),
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
