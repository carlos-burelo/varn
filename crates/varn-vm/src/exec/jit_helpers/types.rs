//! Runtime type questions: `typeof`, `instanceof`, array-ness, and enum
//! tag extraction.

use super::construct::jit_propagate_error;
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;

pub(crate) extern "C" fn jit_typeof_val(ctx: *mut ExecCtx, v_tag: u64, v_payload: u64) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let v = VmValue::from_raw_parts(v_tag, v_payload);
        let s = crate::exec::advanced::typeof_val(v, &ctx_ref.heap);
        ctx_ref.jit_native_result = ctx_ref.heap.alloc_str(s);
    }
}

pub(crate) extern "C" fn jit_instanceof(
    ctx: *mut ExecCtx,
    a_tag: u64,
    a_payload: u64,
    b_tag: u64,
    b_payload: u64,
) -> u64 {
    unsafe {
        let ctx_ref = &*ctx;
        let a = VmValue::from_raw_parts(a_tag, a_payload);
        let b = VmValue::from_raw_parts(b_tag, b_payload);
        let r = crate::exec::advanced::instanceof(a, b, &ctx_ref.heap);
        if r {
            1
        } else {
            0
        }
    }
}

pub(crate) extern "C" fn jit_get_enum_tag(ctx: *mut ExecCtx, val_tag: u64, val_payload: u64) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let val = VmValue::from_raw_parts(val_tag, val_payload);
        match crate::exec::advanced::get_enum_tag(val, &ctx_ref.heap) {
            Ok(tag_val) => ctx_ref.jit_native_result = tag_val,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

/// Whether a boxed value is truthy: the interpreter's branch condition
/// (`VmValue::is_truthy`), for compiled code branching on a value that is not
/// a `bool`.
pub(crate) extern "C" fn jit_truthy(tag: u64, payload: u64) -> u64 {
    u64::from(VmValue::from_raw_parts(tag, payload).is_truthy())
}

pub(crate) extern "C" fn jit_is_array(ctx: *mut ExecCtx, val_tag: u64, val_payload: u64) -> u64 {
    unsafe {
        let ctx_ref = &*ctx;
        let val = VmValue::from_raw_parts(val_tag, val_payload);
        if crate::exec::advanced::is_array(val, &ctx_ref.heap) {
            1
        } else {
            0
        }
    }
}

/// `MakeEnumVariant` out of the lowering from bytecode: the operands follow
/// the opcode at `ip_before` of the running closure's code.
pub(crate) extern "C" fn jit_make_enum_variant(ctx: *mut ExecCtx, ip_before: usize) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let frame_idx = ctx_ref.frames.len() - 1;
        let closure_ref = ctx_ref.frames[frame_idx].closure();
        let base = ctx_ref.frames[frame_idx].base;
        let code = &closure_ref.proto.chunk.code;
        let tag_reg = (code[ip_before] & 0xFF) as usize;
        let name_nv = closure_ref.constants[code[ip_before + 1] as usize];
        let name = ctx_ref.heap.str_val(name_nv).expect("non-string const");
        let tag = ctx_ref.stack.box_reg(base, tag_reg).as_int();
        ctx_ref.jit_native_result = ctx_ref.make_enum_variant(tag, name.as_ref());
    }
}

/// `MakeEnumVariant` out of the lowering from typed SSA: discriminant `tag`
/// and the running closure's string constant `meta_idx` as the descriptor.
pub(crate) extern "C" fn jit_make_enum_variant_const(
    ctx: *mut ExecCtx,
    closure: *const crate::closure::VmClosure,
    tag: i64,
    meta_idx: usize,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let closure_ref = &*closure;
        let name_nv = closure_ref.constants[meta_idx];
        let name = ctx_ref.heap.str_val(name_nv).expect("non-string const");
        ctx_ref.jit_native_result = ctx_ref.make_enum_variant(tag, name.as_ref());
    }
}

/// A numeric conversion (`as`) of a boxed value by the runtime's one
/// `convert`; a failed conversion throws.
pub(crate) extern "C" fn jit_convert(ctx: *mut ExecCtx, conv: u64, tag: u64, payload: u64) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let v = VmValue::from_raw_parts(tag, payload);
        let r = match varn_core::NumConv::from_u8(conv as u8) {
            Some(conv) => crate::exec::convert::convert(conv, v, &mut ctx_ref.heap),
            None => Err(crate::error::RuntimeError::new("convert: bad operand")),
        };
        match r {
            Ok(v) => ctx_ref.jit_native_result = v,
            Err(e) => super::construct::jit_propagate_error(ctx_ref, e),
        }
    }
}
