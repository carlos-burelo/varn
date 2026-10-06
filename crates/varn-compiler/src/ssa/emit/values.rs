#[path = "values/access.rs"]
mod access;
#[path = "values/build.rs"]
mod build;
#[path = "values/calls.rs"]
mod calls;
#[path = "values/closure.rs"]
mod closure;
#[path = "values/scalars.rs"]
mod scalars;
use super::super::ir::{BlockId, Inst, InstKind};
use crate::OptError;
use std::sync::Arc;
use varn_core::OpCode;
use varn_types::chunk::Chunk;
type Result<T> = std::result::Result<T, OptError>;

#[allow(clippy::too_many_arguments)]
pub(super) fn emit_value(
    chunk: &mut Chunk,
    inst: &Inst,
    d: u8,
    value_tys: &[crate::hir::HirType],
    reg: &[u8],
    scratch: u8,
    call_base: u8,
    ic_slot: Option<u8>,
    source_file: &Arc<str>,
    nparams: usize,
    fixups: &mut Vec<(usize, BlockId)>,
    closure_const: &mut Option<u16>,
) -> Result<()> {
    let line = inst.line;
    match &inst.kind {
        InstKind::ConstInt(n) => scalars::emit_const_int(chunk, d, *n, line),
        InstKind::ConstFloat(f) => scalars::emit_const_float(chunk, d, *f, line),
        InstKind::ConstBool(b) => scalars::emit_const_bool(chunk, d, *b, line),
        InstKind::ConstStr(s) => scalars::emit_const_str(chunk, d, s, line),
        InstKind::ConstChar(c) => scalars::emit_const_char(chunk, d, *c, line),
        InstKind::ConstDecimal(dec) => scalars::emit_const_decimal(chunk, d, dec, line),
        InstKind::ConstBigInt(n) => scalars::emit_const_bigint(chunk, d, n, line)?,
        InstKind::ConstNull => scalars::emit_const_null(chunk, d, line),
        InstKind::Binary { op, lhs, rhs, ty } => {
            scalars::emit_binary(chunk, d, *op, *lhs, *rhs, *ty, value_tys, reg, line);
        }
        InstKind::Unary { op, operand, .. } => {
            scalars::emit_unary(chunk, d, *op, *operand, reg, scratch, line);
        }
        InstKind::LoadGlobal(name) => access::emit_load_global(chunk, d, name, line),
        InstKind::LoadGlobalIdx(slot) => access::emit_load_global_idx(chunk, d, *slot, line)?,
        InstKind::LoadNativeGlobalIdx(slot) => {
            access::emit_load_native_global_idx(chunk, d, *slot, line)?;
        }
        InstKind::LoadUpvalue(uv) => access::emit_load_upvalue(chunk, d, *uv, line),

        InstKind::Call { callee, args } => {
            calls::emit_call(chunk, d, *callee, args, reg, call_base, line);
        }
        InstKind::AllocInstance { class } => {
            chunk.write(Chunk::pack_op(OpCode::AllocInstance, d), line);
            chunk.write(Chunk::pack(reg[class.0 as usize], 0), line);
        }

        InstKind::SelfCall { args } => {
            calls::emit_self_call(chunk, d, args, reg, call_base, line);
        }
        InstKind::GetProperty { object, name } => {
            access::emit_get_property(chunk, d, *object, name, reg, ic_slot, line)?;
        }
        InstKind::GetFixedField {
            object,
            slot,
            offset,
            tag,
        } => {
            access::emit_get_fixed_field(chunk, d, *object, reg, *slot, *offset, *tag, line);
        }
        InstKind::GetIndex { object, index } => {
            access::emit_get_index(chunk, d, *object, *index, reg, line);
        }
        InstKind::ArrayGetIndex { object, index } => {
            access::emit_array_get_index(chunk, d, *object, *index, reg, line);
        }
        InstKind::MapGetIndex { object, index } => {
            access::emit_map_get_index(chunk, d, *object, *index, reg, line);
        }

        InstKind::MethodCall { recv, name, args } => {
            calls::emit_method_call(
                chunk, d, *recv, name, args, value_tys, reg, call_base, ic_slot, line,
            )?;
        }
        InstKind::IsNull { operand } => {
            scalars::emit_is_null(chunk, d, *operand, reg, line);
        }
        InstKind::Cast { operand, .. } => {
            scalars::emit_cast(chunk, d, *operand, reg, line);
        }
        InstKind::Convert { operand, conv } => {
            scalars::emit_convert(chunk, d, *operand, reg, *conv, line);
        }

        InstKind::BuildArray { elements } => {
            build::emit_build_array(chunk, d, elements, reg, call_base, line);
        }

        InstKind::BuildTuple { elements } => {
            build::emit_build_tuple(chunk, d, elements, reg, call_base, line);
        }

        InstKind::BuildObject { pairs } => {
            build::emit_build_object(chunk, d, pairs, reg, call_base, line);
        }

        InstKind::BuildRecord { pairs } => {
            build::emit_build_record(chunk, d, pairs, reg, call_base, line);
        }
        InstKind::BuildMap { pairs } => {
            build::emit_build_map(chunk, d, pairs, reg, call_base, line);
        }
        InstKind::ToString { operand } => {
            scalars::emit_to_string(chunk, d, *operand, reg, line);
        }

        InstKind::MakeClosure { func, upvalues_src } => {
            closure::emit_make_closure(
                chunk,
                d,
                *func,
                upvalues_src,
                source_file,
                nparams,
                closure_const,
                line,
            )?;
        }

        InstKind::IntrinsicCall {
            object,
            args,
            wire_byte,
        } if args.len() == 1
            && varn_core::intrinsic_ops::math::is_unary_math(*wire_byte)
            && matches!(
                value_tys.get(args[0].0 as usize),
                Some(crate::hir::HirType::Float)
            ) =>
        {
            calls::emit_intrinsic_direct(chunk, d, args, *wire_byte, reg, line);
        }

        InstKind::IntrinsicCall {
            object,
            args,
            wire_byte,
        } => {
            calls::emit_intrinsic(chunk, d, *object, args, *wire_byte, reg, call_base, line);
        }

        InstKind::CallNativeOp {
            object,
            args,
            op_id,
        } => {
            calls::emit_call_native_op(chunk, d, *object, args, *op_id, reg, call_base, line);
        }

        InstKind::BuildStr { parts } => {
            build::emit_build_str(chunk, d, parts, reg, line);
        }
        InstKind::GetPropertyMaybe { object, name } => {
            access::emit_get_property_maybe(chunk, d, *object, name, reg, line);
        }
        InstKind::ModuleSlot { object, slot } => {
            access::emit_module_slot(chunk, d, *object, reg, *slot, line);
        }
        InstKind::GetEnumTag { operand } => {
            scalars::emit_get_enum_tag(chunk, d, *operand, reg, line);
        }
        InstKind::IsArray { operand } => {
            scalars::emit_is_array(chunk, d, *operand, reg, line);
        }
        InstKind::StrLength { operand } => {
            scalars::emit_str_length(chunk, d, *operand, reg, line);
        }
        InstKind::ArrayLength { operand } => {
            scalars::emit_array_length(chunk, d, *operand, reg, line);
        }
        InstKind::BytesLength { operand } => {
            scalars::emit_bytes_length(chunk, d, *operand, reg, line);
        }

        InstKind::This => scalars::emit_this(chunk, d, line),

        InstKind::Range {
            start,
            end,
            inclusive,
        } => {
            access::emit_range(chunk, d, *start, *end, *inclusive, reg, line);
        }
        InstKind::ObjectKeys { operand } => {
            scalars::emit_object_keys(chunk, d, *operand, reg, line);
        }
        InstKind::GetSymbol { object, is_async } => {
            scalars::emit_get_symbol(chunk, d, *object, reg, *is_async, line);
        }

        InstKind::IterCall { callee, recv } => {
            calls::emit_iter_call(chunk, d, *callee, *recv, reg, call_base, line);
        }

        InstKind::GetSuper { name } => {
            access::emit_get_super(chunk, d, name, line);
        }

        InstKind::SuperCall { args } => {
            calls::emit_super_call(chunk, d, args, reg, call_base, line);
        }

        InstKind::SuperMethodCall { name, args } => {
            calls::emit_super_method_call(chunk, d, name, args, reg, call_base, line);
        }

        InstKind::ExtensionCall {
            func,
            slot,
            recv,
            args,
        } => {
            calls::emit_extension_call(chunk, d, func, *slot, *recv, args, reg, call_base, line)?;
        }

        InstKind::CallSpread { callee, args } => {
            calls::emit_call_spread(chunk, d, *callee, args, reg, call_base, line);
        }

        InstKind::BuildArraySpread { elements } => {
            build::emit_build_array_spread(chunk, d, elements, reg, call_base, line);
        }

        InstKind::BuildObjectSpread { parts } => {
            build::emit_build_object_spread(chunk, d, parts, reg, ic_slot, line)?;
        }
        InstKind::ObjectRest { object, skip_keys } => {
            build::emit_object_rest(chunk, d, *object, skip_keys, reg, line);
        }
        InstKind::LoadCaptured { var } => {
            access::emit_load_captured(chunk, d, *var, nparams, line);
        }
        InstKind::MakeClass { name, super_class } => {
            closure::emit_make_class(chunk, d, name, *super_class, reg, line);
        }
        InstKind::MakeEnumVariant { tag, meta } => {
            closure::emit_make_enum_variant(chunk, d, *tag, meta, scratch, line);
        }
        InstKind::Try { handler } => {
            closure::emit_try(chunk, d, *handler, fixups, line);
        }
        InstKind::CatchParam { try_val } => {
            closure::emit_catch_param(chunk, d, *try_val, reg, line);
        }
        InstKind::LoadModule { source } => {
            access::emit_load_module(chunk, d, source, line);
        }
        InstKind::Await { operand } => {
            closure::emit_await(chunk, d, *operand, reg, line);
        }
        InstKind::Spawn { operand } => {
            closure::emit_spawn(chunk, d, *operand, reg, line);
        }
        InstKind::Yield { operand } => {
            closure::emit_yield(chunk, d, *operand, reg, line);
        }

        InstKind::SetProperty { .. }
        | InstKind::SetFixedField { .. }
        | InstKind::SetIndex { .. }
        | InstKind::ArrayPush { .. }
        | InstKind::ArraySetIndex { .. }
        | InstKind::MapSetIndex { .. }
        | InstKind::ObjectMerge { .. }
        | InstKind::AssertNotNull { .. }
        | InstKind::StoreGlobal { .. }
        | InstKind::StoreGlobalIdx { .. }
        | InstKind::StoreUpvalue { .. }
        | InstKind::StoreCaptured { .. }
        | InstKind::StoreModuleSlot { .. }
        | InstKind::CloseUpvalues { .. }
        | InstKind::Dispose { .. }
        | InstKind::PopTry
        | InstKind::DeclareLayout { .. }
        | InstKind::DefineStatic { .. }
        | InstKind::DefineMethod { .. }
        | InstKind::DefineAccessor { .. } => {
            unreachable!()
        }
    }
    Ok(())
}
