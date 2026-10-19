use super::construct::jit_propagate_error;
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;

#[varn_macros::jit_slow(field = "typeof_val")]
pub(crate) extern "C" fn jit_typeof_val(ctx: *mut ExecCtx, v_tag: u64, v_payload: u64) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let v = VmValue::from_raw_parts(v_tag, v_payload);
        let s = crate::exec::advanced::typeof_val(v, &ctx_ref.heap);
        ctx_ref.jit_native_result = ctx_ref.heap.alloc_str(s);
    }
}

#[varn_macros::jit_slow(field = "instanceof")]
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

#[varn_macros::jit_slow(field = "get_enum_tag")]
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

#[varn_macros::jit_slow(field = "truthy")]
pub(crate) extern "C" fn jit_truthy(tag: u64, payload: u64) -> u64 {
    u64::from(VmValue::from_raw_parts(tag, payload).is_truthy())
}

#[varn_macros::jit_slow(field = "is_array")]
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

#[varn_macros::jit_slow(field = "make_enum_variant")]
pub(crate) extern "C" fn jit_make_enum_variant(ctx: *mut ExecCtx, ip_before: usize) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let frame_idx = ctx_ref.frames.len() - 1;
        let proto = std::rc::Rc::clone(&ctx_ref.frames[frame_idx].closure().proto);
        let def_idx = proto.chunk.code[ip_before + 1] as usize;
        ctx_ref.jit_native_result = ctx_ref
            .make_enum_variant(&proto.chunk.constants, def_idx)
            .expect("MakeEnumVariant constant is a variant");
    }
}

#[varn_macros::jit_slow(field = "make_enum_variant_const")]
pub(crate) extern "C" fn jit_make_enum_variant_const(
    ctx: *mut ExecCtx,
    closure: *const crate::closure::VmClosure,
    def_idx: usize,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let proto = std::rc::Rc::clone(&(*closure).proto);
        ctx_ref.jit_native_result = ctx_ref
            .make_enum_variant(&proto.chunk.constants, def_idx)
            .expect("MakeEnumVariant constant is a variant");
    }
}

#[varn_macros::jit_slow(field = "convert")]
pub(crate) extern "C" fn jit_convert(ctx: *mut ExecCtx, conv: u64, tag: u64, payload: u64) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let v = VmValue::from_raw_parts(tag, payload);
        let r = match varn_core::ValueConv::from_u8(conv as u8) {
            Some(conv) => crate::exec::convert::convert(conv, v, &mut ctx_ref.heap),
            None => Err(crate::error::RuntimeError::new("convert: bad operand")),
        };
        match r {
            Ok(v) => ctx_ref.jit_native_result = v,
            Err(e) => super::construct::jit_propagate_error(ctx_ref, e),
        }
    }
}
