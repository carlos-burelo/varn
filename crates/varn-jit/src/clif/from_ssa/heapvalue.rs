//! Heap-producing instructions of the SSA lowering.
//!
//! Every arm here writes its result to the destination value's **home** (the GC
//! root), or unboxes a boxed helper result into a scalar destination, or writes
//! through its operands with no result. They are split from [`super::scalar`]
//! so each file owns one invariant (scalar register file vs heap homes) and
//! neither crosses the file-size limit.

use cranelift_codegen::ir::{InstBuilder, Value};
use cranelift_frontend::FunctionBuilder;
use varn_types::ssa::SsaOp;

use super::{classops, def_heap, globals, heap, is_heap, load_value, props, Ctx};
use super::super::emit::unbox_int;

/// Emit a heap instruction. `Ok(None)` means `op` is not a heap instruction
/// and the caller's scalar path must handle it.
pub(super) fn emit(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    values: &mut [Option<Value>],
    op: &SsaOp,
    dest: Option<u32>,
) -> Result<Option<Option<Value>>, String> {
    // Void heap ops: no result.
    match op {
        SsaOp::SetIndex {
            object,
            index,
            value,
        } => {
            props::emit_set_index(b, ctx, values, *object, *index, *value)?;
            return Ok(Some(None));
        }
        SsaOp::ArrayPush { array, value } => {
            props::emit_array_push(b, ctx, values, *array, *value)?;
            return Ok(Some(None));
        }
        SsaOp::SetFixedField {
            object,
            value,
            slot,
        } => {
            props::emit_set_fixed_field(b, ctx, values, *object, *value, *slot)?;
            return Ok(Some(None));
        }
        SsaOp::SetProperty {
            object,
            value,
            name,
            cs,
        } => {
            props::emit_set_property(b, ctx, values, *object, *value, name, *cs)?;
            return Ok(Some(None));
        }
        SsaOp::DeclareField { class, name, tag } => {
            classops::emit_declare_field(b, ctx, values, *class, name, *tag)?;
            return Ok(Some(None));
        }
        SsaOp::DefineMethod {
            class,
            name,
            member,
            kind,
        } => {
            classops::emit_define_member(b, ctx, values, *class, name, *member, *kind as i64)?;
            return Ok(Some(None));
        }
        _ => {}
    }

    let boxed_home = |b: &mut FunctionBuilder,
                      ctx: &Ctx<'_>,
                      boxed: Value|
     -> Result<Value, String> {
        let d = dest.ok_or("from_ssa: heap result without dest")?;
        def_heap(b, ctx, ctx.ssa.reg(d), boxed)?;
        Ok(boxed)
    };

    let v = match op {
        SsaOp::ConstNull => {
            let tag = b.ins().iconst(
                cranelift_codegen::ir::types::I64,
                varn_types::vm_value::KIND_NULL as i64,
            );
            let payload = b.ins().iconst(cranelift_codegen::ir::types::I64, 0);
            let boxed = b.ins().iconcat(tag, payload);
            boxed_home(b, ctx, boxed)?
        }
        SsaOp::ConstStr(s) => {
            let boxed = const_str(b, ctx, s)?;
            boxed_home(b, ctx, boxed)?
        }
        SsaOp::LoadGlobalIdx(slot) => {
            let boxed = globals::emit_load(b, ctx, *slot)?;
            boxed_home(b, ctx, boxed)?
        }
        SsaOp::This => {
            let boxed = props::emit_this(b, ctx)?;
            boxed_home(b, ctx, boxed)?
        }
        SsaOp::Binary {
            op: varn_types::ssa::SsaBinOp::StrConcat,
            lhs,
            rhs,
        } => {
            let boxed = heap::emit_str_concat(b, ctx, values, *lhs, *rhs)?;
            boxed_home(b, ctx, boxed)?
        }
        SsaOp::BuildStr { parts } => {
            let boxed = heap::emit_build_str(b, ctx, values, parts)?;
            boxed_home(b, ctx, boxed)?
        }
        SsaOp::BuildArray { elements } => {
            let boxed = heap::emit_build_array(b, ctx, values, elements)?;
            boxed_home(b, ctx, boxed)?
        }
        SsaOp::BuildMap { pairs } => {
            let boxed = heap::emit_build_map(b, ctx, values, pairs)?;
            boxed_home(b, ctx, boxed)?
        }
        SsaOp::BuildObject {
            keys,
            values: ids,
            is_record,
        } => {
            let boxed = heap::emit_build_object(b, ctx, values, ids, keys, *is_record)?;
            boxed_home(b, ctx, boxed)?
        }
        SsaOp::GetIndex { object, index } => {
            let boxed = props::emit_get_index(b, ctx, values, *object, *index)?;
            let d = dest.ok_or("from_ssa: get_index without dest")?;
            let dt = ctx.ssa.value_ty(d);
            if is_heap(dt) {
                def_heap(b, ctx, ctx.ssa.reg(d), boxed)?;
                boxed
            } else {
                heap::unbox_dest(b, dt, boxed)?
            }
        }
        SsaOp::GetFixedField { object, slot } => {
            let boxed = props::emit_get_fixed_field(b, ctx, values, *object, *slot)?;
            let d = dest.ok_or("from_ssa: get_fixed_field without dest")?;
            let dt = ctx.ssa.value_ty(d);
            if is_heap(dt) {
                def_heap(b, ctx, ctx.ssa.reg(d), boxed)?;
                boxed
            } else {
                heap::unbox_dest(b, dt, boxed)?
            }
        }
        SsaOp::GetProperty { object, name, cs } => {
            let d = dest.ok_or("from_ssa: get_property without dest")?;
            let dest_reg = ctx.ssa.reg(d);
            let boxed = props::emit_get_property(b, ctx, values, *object, name, *cs, dest_reg)?;
            let dt = ctx.ssa.value_ty(d);
            if is_heap(dt) {
                boxed
            } else {
                heap::unbox_dest(b, dt, boxed)?
            }
        }
        SsaOp::MakeClass { name, super_class } => {
            let boxed = classops::emit_make_class(b, ctx, values, name, *super_class)?;
            boxed_home(b, ctx, boxed)?
        }
        SsaOp::GetSuper { name } => {
            let boxed = classops::emit_get_super(b, ctx, name)?;
            boxed_home(b, ctx, boxed)?
        }
        SsaOp::Typeof { operand } => {
            let boxed = heap::emit_unary_boxed(b, ctx, values, *operand, ctx.helpers.typeof_val)?;
            boxed_home(b, ctx, boxed)?
        }
        SsaOp::ToString { operand } => {
            let boxed = heap::emit_unary_boxed(b, ctx, values, *operand, ctx.helpers.to_string)?;
            boxed_home(b, ctx, boxed)?
        }
        SsaOp::ObjectKeys { operand } => {
            let boxed = heap::emit_unary_boxed(b, ctx, values, *operand, ctx.helpers.object_keys)?;
            boxed_home(b, ctx, boxed)?
        }
        SsaOp::IsArray { operand } => heap::emit_is_array(b, ctx, values, *operand)?,
        SsaOp::GetEnumTag { operand } => {
            let boxed =
                heap::emit_unary_boxed(b, ctx, values, *operand, ctx.helpers.get_enum_tag)?;
            unbox_int(b, boxed)
        }
        SsaOp::ArrayLength { operand } => {
            let boxed = props::emit_array_length(b, ctx, values, *operand)?;
            unbox_int(b, boxed)
        }
        SsaOp::IsNull { operand } => {
            let a = load_value(b, ctx, values, *operand)?;
            let (tag, _) = b.ins().isplit(a);
            let c = b.ins().icmp_imm(
                cranelift_codegen::ir::condcodes::IntCC::Equal,
                tag,
                varn_types::vm_value::KIND_NULL as i64,
            );
            b.ins().uextend(cranelift_codegen::ir::types::I64, c)
        }
        _ => return Ok(None),
    };
    Ok(Some(Some(v)))
}

/// A string literal's resolved `VmValue`, found in the proto's pool (1:1 with
/// the resolved constants). Interned, so the handle is the same one the
/// bytecode `LoadConst` would bake.
fn const_str(
    b: &mut FunctionBuilder,
    ctx: &Ctx<'_>,
    s: &str,
) -> Result<Value, String> {
    use varn_types::{Literal, PoolEntry};
    let idx = ctx
        .proto
        .chunk
        .constants
        .iter()
        .position(|e| matches!(e, PoolEntry::Literal(Literal::Str(t)) if t.as_ref() == s))
        .ok_or("from_ssa: string constant not in pool")?;
    let cv = ctx
        .constants
        .get(idx)
        .ok_or("from_ssa: unresolved string constant")?;
    let tag = b.ins().iconst(cranelift_codegen::ir::types::I64, cv.raw_tag() as i64);
    let payload = b
        .ins()
        .iconst(cranelift_codegen::ir::types::I64, cv.raw_payload() as i64);
    Ok(b.ins().iconcat(tag, payload))
}
