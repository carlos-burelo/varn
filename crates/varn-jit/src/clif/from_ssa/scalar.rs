use super::scalar_ops::{emit_bin, emit_un};
use cranelift_codegen::ir::{types, InstBuilder, Value};
use cranelift_frontend::FunctionBuilder;
use varn_types::register_meta::SlotKind;
use varn_types::ssa::{SsaBinOp, SsaOp, SsaUnOp};

use super::{
    arrays, bytes, call, closures, dynop, exceptions, globals, heap, heapvalue, is_heap,
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
        SsaOp::BytesGetIndex { object, index } => {
            return Ok(Some(bytes::emit_get(
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
        SsaOp::BytesSetIndex {
            object,
            index,
            value,
        } => {
            bytes::emit_set(b, ctx, values, *object, *index, *value)?;
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
            let no_overflow = dest.is_some_and(|d| ctx.no_overflow.contains(&d));
            emit_bin(b, ctx, *op, a, c, in_range, no_overflow)?
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
            super::stack_exit::guard_headroom(b, ctx);
            super::stack_exit::enter_frameless(b, ctx);
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
            let result = b.inst_results(call)[0];
            super::stack_exit::leave_frameless(b, ctx);
            result
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
            return Ok(Some(Out::Boxed(
                super::alloc_instance::emit_alloc_instance(b, ctx, values, *class, *payload_size)?,
            )))
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

        SsaOp::CatchParam { try_val } => {
            let boxed = exceptions::emit_catch_param(values, *try_val)?;
            return Ok(Some(Out::Boxed(boxed)));
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
