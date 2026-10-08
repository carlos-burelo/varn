use cranelift_codegen::ir::{
    condcodes::{FloatCC, IntCC},
    types, InstBuilder, Value,
};
use cranelift_frontend::FunctionBuilder;
use varn_types::register_meta::SlotKind;
use varn_types::ssa::{SsaBinOp, SsaOp, SsaUnOp};

use super::{
    arrays, boxed, call, closures, dynop, exceptions, globals, heap, heapvalue, is_heap,
    load_value, numeric, Ctx, Out,
};

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
    if let Some(out) = super::extra::try_emit(b, ctx, values, op, dest)? {
        return Ok(out);
    }

    let dest_ty = dest.map(|d| ctx.ssa.value_ty(d));
    let v = match op {
        SsaOp::ConstInt(n) => b.ins().iconst(types::I64, *n),
        SsaOp::ConstFloat(f) => b.ins().f64const(*f),
        SsaOp::ConstBool(x) => b.ins().iconst(types::I64, i64::from(*x)),
        SsaOp::Convert { operand, conv } => {
            return Ok(Some(numeric::emit_convert(
                b, ctx, values, *operand, *conv,
            )?))
        }
        SsaOp::ArrayGetIndex { object, index } => {
            return Ok(Some(arrays::emit_get(
                b, ctx, values, *object, *index, dest,
            )?))
        }
        SsaOp::ArraySetIndex {
            object,
            index,
            value,
        } => {
            arrays::emit_set(b, ctx, values, *object, *index, *value)?;
            return Ok(None);
        }
        SsaOp::IntrinsicCall { object, args, wire } => {
            return Ok(Some(numeric::emit_intrinsic(
                b, ctx, values, *object, args, *wire, dest,
            )?))
        }

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
            let in_range = dest.is_some_and(|d| ctx.in_range_steps.contains(&d));
            emit_bin(b, ctx, *op, a, c, in_range)?
        }
        SsaOp::Unary { op, operand } => {
            let a = load_value(b, ctx, values, *operand)?;
            emit_un(b, ctx, *op, a)?
        }
        SsaOp::SelfCall { args } => {
            if ctx.frame.is_some() {
                return Ok(Some(Out::Boxed(call::emit_self_call_framed(
                    b, ctx, values, args,
                )?)));
            }
            let no_closure = b.ins().iconst(types::I64, 0);
            let null_tag = b
                .ins()
                .iconst(types::I64, varn_types::vm_value::KIND_NULL as i64);
            let null_payload = b.ins().iconst(types::I64, 0);
            let mut a = vec![ctx.exec_ctx, no_closure, null_tag, null_payload];
            for v in args {
                a.push(load_value(b, ctx, values, *v)?);
            }
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
                b, ctx, values, *recv, name, args, *cs, dest,
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
        SsaOp::Call { callee, args } => {
            return Ok(Some(call::emit_call(b, ctx, values, *callee, args, dest)?))
        }
        SsaOp::AllocInstance {
            class,
            payload_size,
        } => {
            return Ok(Some(Out::Boxed(super::classops::emit_alloc_instance(
                b,
                ctx,
                values,
                *class,
                *payload_size,
            )?)))
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
        SsaOp::Try {
            catch_ip,
            catch_value,
            live,
        } => {
            exceptions::emit_try(b, ctx, values, *catch_ip, *catch_value, live)?;
            return Ok(None);
        }
        SsaOp::PopTry => {
            exceptions::emit_pop_try(b, ctx)?;
            return Ok(None);
        }

        SsaOp::CatchParam { .. } => {
            return Err("from_ssa: a landing pad reached compiled code".into())
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

        SsaOp::ConstNull
        | SsaOp::ConstStr(_)
        | SsaOp::ConstChar(_)
        | SsaOp::ConstBigInt(_)
        | SsaOp::ConstDecimal(_)
        | SsaOp::MakeEnumVariant { .. }
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
        | SsaOp::BytesLength { .. }
        | SsaOp::StrLength { .. }
        | SsaOp::GetFixedField { .. }
        | SsaOp::SetIndex { .. }
        | SsaOp::ArrayPush { .. }
        | SsaOp::SetFixedField { .. }
        | SsaOp::MakeClass { .. }
        | SsaOp::DeclareLayout { .. }
        | SsaOp::DefineMethod { .. }
        | SsaOp::GetSuper { .. }
        | SsaOp::LoadGlobal(_)
        | SsaOp::StoreGlobal { .. }
        | SsaOp::BuildTuple { .. }
        | SsaOp::BuildArraySpread { .. }
        | SsaOp::BuildObjectSpread { .. }
        | SsaOp::ObjectMerge { .. }
        | SsaOp::ObjectRest { .. }
        | SsaOp::GetPropertyMaybe { .. }
        | SsaOp::AssertNotNull { .. }
        | SsaOp::BindMethod { .. }
        | SsaOp::ArrayExtend { .. }
        | SsaOp::WrapSpread { .. }
        | SsaOp::Range { .. }
        | SsaOp::GetSymbol { .. }
        | SsaOp::IterCall { .. }
        | SsaOp::SuperCall { .. }
        | SsaOp::SuperMethodCall { .. }
        | SsaOp::ExtensionCall { .. }
        | SsaOp::CallSpread { .. }
        | SsaOp::LoadModule { .. }
        | SsaOp::ModuleSlot { .. }
        | SsaOp::StoreModuleSlot { .. }
        | SsaOp::Await { .. }
        | SsaOp::Spawn { .. }
        | SsaOp::Yield { .. }
        | SsaOp::Dispose { .. } => {
            unreachable!("heap/extra ops are emitted above")
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

    in_range: bool,
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
        IntAdd if in_range => b.ins().iadd(a, c),
        IntSub if in_range => b.ins().isub(a, c),
        IntAdd | IntSub | IntMul => checked_int(b, ctx, op, a, c),
        IntAnd => b.ins().band(a, c),
        IntOr => b.ins().bor(a, c),
        IntXor => b.ins().bxor(a, c),
        IntShl => {
            let sh = b.ins().band_imm_u(c, 0x3F);
            b.ins().ishl(a, sh)
        }
        IntShr => {
            let sh = b.ins().band_imm_u(c, 0x3F);
            b.ins().sshr(a, sh)
        }
        IntUshr => {
            let sh = b.ins().band_imm_u(c, 0x3F);
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
        IntDiv | IntMod | IntPow | FloatMod | FloatPow => unreachable!("lowered above"),
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
        SsaBinOp::IntMul | SsaBinOp::IntDiv | SsaBinOp::IntMod | SsaBinOp::IntPow | SsaBinOp::IntEq | SsaBinOp::IntNe | SsaBinOp::IntLt | SsaBinOp::IntLe | SsaBinOp::IntGt | SsaBinOp::IntGe | SsaBinOp::IntAnd | SsaBinOp::IntOr | SsaBinOp::IntXor | SsaBinOp::IntShl | SsaBinOp::IntShr | SsaBinOp::IntUshr | SsaBinOp::FloatAdd | SsaBinOp::FloatSub | SsaBinOp::FloatMul | SsaBinOp::FloatDiv | SsaBinOp::FloatMod | SsaBinOp::FloatPow | SsaBinOp::FloatEq | SsaBinOp::FloatNe | SsaBinOp::FloatLt | SsaBinOp::FloatLe | SsaBinOp::FloatGt | SsaBinOp::FloatGe | SsaBinOp::StrConcat | SsaBinOp::Dyn(_) => {
            let (r, o) = b.ins().smul_overflow(a, c);
            (r, o, helpers.mul)
        }
    };

    guard_overflow(b, ctx.cc, ctx.exec_ctx, helper, r, ovf, a, c)
}

fn emit_un(b: &mut FunctionBuilder, ctx: &Ctx<'_>, op: SsaUnOp, a: Value) -> Result<Value, String> {
    use super::super::emit::{box_int, call_helper_void};
    Ok(match op {
        SsaUnOp::NegInt => {
            let cc = ctx.cc;
            let helpers = ctx.helpers;
            let neg = b.ins().ineg(a);
            let fits = b.ins().icmp_imm_u(IntCC::NotEqual, a, i64::MIN);
            let raise = b.create_block();
            let cont = b.create_block();
            b.ins().brif(fits, cont, &[], raise, &[]);
            b.switch_to_block(raise);
            let boxed = box_int(b, a);
            let (tag, payload) = b.ins().isplit(boxed);
            call_helper_void(b, cc, helpers.negate, &[ctx.exec_ctx, tag, payload]);
            b.ins().jump(cont, &[]);
            b.switch_to_block(cont);
            neg
        }
        SsaUnOp::NegFloat => b.ins().fneg(a),
        SsaUnOp::Not => b.ins().bxor_imm_u(a, 1),
        SsaUnOp::BitNotInt => b.ins().bxor_imm_u(a, -1),
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
