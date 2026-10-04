//! Shared CLIF emission primitives: value coercion (box/unbox), the
//! runtime-helper call shim, and the relocation patcher. The SSA lowering is
//! the only consumer; the bytecode register-variable helpers died with it.

use cranelift_codegen::ir::{condcodes::IntCC, types, AbiParam, InstBuilder, Signature};
use cranelift_frontend::{FunctionBuilder, Variable};
use varn_types::register_meta::SlotKind;

mod payload;

pub(in crate::clif) use payload::*;

// Tag tests, re-emitted inline. These apply to a value's TAG WORD; the
// payload word carries the datum and is never masked. Under the NaN-box
// these were 64-bit constants that had to come from a constant pool — as
// small immediates they now fold into the compare.
pub(super) const KIND_MASK: i64 = varn_types::vm_value::KIND_MASK as i64;
pub(super) const HEAP_KIND: i64 = varn_types::vm_value::KIND_HEAP as i64;

/// `site` is the buffer offset of the rel32 field; `target` the buffer
/// offset the call must reach.
pub(super) fn patch_rel32(buf: &mut [u8], site: usize, target: usize) {
    let disp = target as i64 - (site as i64 + 4);
    let disp = i32::try_from(disp).expect("clif: rel32 out of range");
    buf[site..site + 4].copy_from_slice(&disp.to_le_bytes());
}

/// Re-tag an unboxed int as a VmValue.
// Parameters kept, unused: they document the signature the migrated
// version has to satisfy.
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

/// Box an unboxed 0/1 bool as a VmValue: `false` = 0x7FFA…, `true` = 0x7FFB…
/// (`0x7FFA_0000_0000_0000 | (v << 48)`, valid because v ∈ {0,1}).
// Parameters kept, unused: they document the signature the migrated
// version has to satisfy.
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

/// Unbox a boxed bool VmValue (TAG_TRUE=0x7FFB…, TAG_FALSE=0x7FFA…) to 0/1:
/// the two tags differ only in bit 48, so `(v >> 48) & 1`.
// Parameters kept, unused: they document the signature the migrated
// version has to satisfy.
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

/// Box an unboxed `f64` as a VmValue, replicating `VmValue::from_f64`: a
/// float is stored as its raw bits EXCEPT a quiet-NaN-range result
/// (`bits & QNAN == QNAN`) canonicalizes to `null`. This keeps a native float
/// result byte-identical to the interpreter, which routes every float op
/// through `from_f64`.
// Parameters kept, unused: they document the signature the migrated
// version has to satisfy.
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

/// Unbox a VmValue a float slot may hold as EITHER float bits OR an int
/// VmValue — the latter arises when a widening int argument is passed to a
/// float parameter (`takesFloat(5)`), where the caller boxes the int and the
/// callee must coerce, exactly as the interpreter's `to_f64_val` does. An
/// int-tagged value `fcvt`s its raw i64 payload; anything else is
/// reinterpreted as its f64 bits.
// Parameters kept, unused: they document the signature the migrated
// version has to satisfy.
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

/// Whether register `r` is float-typed (its Variable is declared `F64`).
pub(super) fn meta_is_float(meta: &[varn_types::register_meta::RegisterMeta], r: usize) -> bool {
    meta.get(r).is_some_and(|m| m.kind == SlotKind::Float)
}

/// The null `VmValue` bits.
pub(super) fn box_null(b: &mut FunctionBuilder) -> cranelift_codegen::ir::Value {
    let tag = b
        .ins()
        .iconst(types::I64, varn_types::vm_value::KIND_NULL as i64);
    let zero = b.ins().iconst(types::I64, 0);
    b.ins().iconcat(tag, zero)
}
