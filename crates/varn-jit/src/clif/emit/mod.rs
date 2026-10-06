use cranelift_codegen::ir::{condcodes::IntCC, types, AbiParam, InstBuilder, Signature};
use cranelift_frontend::FunctionBuilder;
use varn_types::register_meta::SlotKind;

mod payload;

pub(in crate::clif) use payload::*;

pub(super) const KIND_MASK: i64 = varn_types::vm_value::KIND_MASK as i64;
pub(super) const HEAP_KIND: i64 = varn_types::vm_value::KIND_HEAP as i64;

pub(super) fn patch_rel32(buf: &mut [u8], site: usize, target: usize) {
    let disp = target as i64 - (site as i64 + 4);
    let disp = i32::try_from(disp).expect("clif: rel32 out of range");
    buf[site..site + 4].copy_from_slice(&disp.to_le_bytes());
}

#[allow(unused_variables)]
pub(super) fn box_int(
    b: &mut FunctionBuilder,
    v: cranelift_codegen::ir::Value,
) -> cranelift_codegen::ir::Value {
    let tag = b
        .ins()
        .iconst(types::I64, varn_types::vm_value::KIND_INT as i64);
    b.ins().iconcat(tag, v)
}

pub(super) fn box_bool(
    b: &mut FunctionBuilder,
    v: cranelift_codegen::ir::Value,
) -> cranelift_codegen::ir::Value {
    let tag = b
        .ins()
        .iconst(types::I64, varn_types::vm_value::KIND_BOOL as i64);
    let payload = if b.func.dfg.value_type(v) == types::I64 {
        v
    } else {
        b.ins().uextend(types::I64, v)
    };
    b.ins().iconcat(tag, payload)
}

pub(super) fn unbox_bool(
    b: &mut FunctionBuilder,
    v: cranelift_codegen::ir::Value,
) -> cranelift_codegen::ir::Value {
    if b.func.dfg.value_type(v) == types::I128 {
        let (_tag, payload) = b.ins().isplit(v);
        payload
    } else {
        v
    }
}

pub(super) fn box_f64(
    b: &mut FunctionBuilder,
    v: cranelift_codegen::ir::Value,
) -> cranelift_codegen::ir::Value {
    let tag = b
        .ins()
        .iconst(types::I64, varn_types::vm_value::KIND_FLOAT as i64);
    let payload = b
        .ins()
        .bitcast(types::I64, cranelift_codegen::ir::MemFlagsData::new(), v);
    b.ins().iconcat(tag, payload)
}

pub(super) fn unbox_f64_coerce(
    b: &mut FunctionBuilder,
    v: cranelift_codegen::ir::Value,
) -> cranelift_codegen::ir::Value {
    let ty = b.func.dfg.value_type(v);
    if ty == types::F64 {
        v
    } else if ty == types::I128 {
        let (tag, payload) = b.ins().isplit(v);
        let is_float = b.ins().icmp_imm_u(
            cranelift_codegen::ir::condcodes::IntCC::Equal,
            tag,
            varn_types::vm_value::KIND_FLOAT as i64,
        );
        let f_direct = b.ins().bitcast(
            types::F64,
            cranelift_codegen::ir::MemFlagsData::new(),
            payload,
        );
        let f_from_int = b.ins().fcvt_from_sint(types::F64, payload);
        b.ins().select(is_float, f_direct, f_from_int)
    } else if ty == types::I64 {
        b.ins().fcvt_from_sint(types::F64, v)
    } else {
        let ext = b.ins().sextend(types::I64, v);
        b.ins().fcvt_from_sint(types::F64, ext)
    }
}

pub(super) fn meta_is_float(meta: &[varn_types::register_meta::RegisterMeta], r: usize) -> bool {
    meta.get(r).is_some_and(|m| m.kind == SlotKind::Float)
}

pub(super) fn box_null(b: &mut FunctionBuilder) -> cranelift_codegen::ir::Value {
    let tag = b
        .ins()
        .iconst(types::I64, varn_types::vm_value::KIND_NULL as i64);
    let zero = b.ins().iconst(types::I64, 0);
    b.ins().iconcat(tag, zero)
}
