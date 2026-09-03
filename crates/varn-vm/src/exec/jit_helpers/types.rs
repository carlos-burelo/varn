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

pub(crate) extern "C" fn jit_is_array_stub(
    ctx: *mut ExecCtx,
    val_tag: u64,
    val_payload: u64,
) -> u64 {
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

pub(crate) extern "C" fn jit_make_enum_variant(ctx: *mut ExecCtx, ip_before: usize) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let frame_idx = ctx_ref.frames.len() - 1;
        let closure_ref = ctx_ref.frames[frame_idx].closure();
        let base = ctx_ref.frames[frame_idx].base;
        let code = &closure_ref.proto.chunk.code;

        let mut temp_ip = ip_before;
        let w1 = code[temp_ip];
        temp_ip += 1;
        let name_idx = code[temp_ip] as usize;
        let tag_reg = (w1 & 0xFF) as usize;
        let name_nv = closure_ref.constants[name_idx];
        let name = ctx_ref.heap.str_val(name_nv).expect("non-string const");
        let tag = ctx_ref.stack[base + tag_reg].as_int();

        let name_str = name.as_ref();
        let (name_part, fields_part) = match name_str.find(':') {
            Some(idx) => (&name_str[..idx], &name_str[idx + 1..]),
            None => (name_str, ""),
        };
        let (enum_name_str, variant_name_str) = match name_part.rfind('.') {
            Some(idx) => (&name_part[..idx], &name_part[idx + 1..]),
            None => ("", name_part),
        };
        let fields: Vec<std::rc::Rc<str>> = if fields_part.is_empty() {
            vec![]
        } else {
            fields_part.split(',').map(std::rc::Rc::from).collect()
        };

        let variant =
            varn_types::Value::EnumVariant(Box::new(varn_types::value::EnumVariantData {
                enum_name: std::rc::Rc::from(enum_name_str),
                variant_name: std::rc::Rc::from(variant_name_str),
                variant_tag: tag,
                fields,
                payload: varn_types::Value::Object(varn_types::value::ObjRef::empty()),
            }));
        ctx_ref.jit_native_result = ctx_ref.heap.intern(variant);
    }
}
