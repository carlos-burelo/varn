//! Aggregate construction from compiled code: array and string literals.

use super::construct::jit_propagate_error;
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;

pub(crate) extern "C" fn jit_build_array(
    ctx: *mut ExecCtx,
    base: usize,
    start_reg: usize,
    count: usize,
) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let mut elems = Vec::with_capacity(count);
        for i in 0..count {
            let nv = ctx_ref.stack[base + start_reg + i];
            elems.push(nv);
        }
        ctx_ref.heap.alloc_array_vm(elems)
    }
}

pub(crate) extern "C" fn jit_build_str(
    ctx: *mut ExecCtx,
    parts_ptr: *const VmValue,
    count: usize,
) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let parts = std::slice::from_raw_parts(parts_ptr, count);
        // Stack-first buffer, exactly as the interpreter's `BuildStr` — a
        // template whose result fits inline never reaches the allocator, and
        // one that does not pays a single `Rc<str>` instead of a scratch
        // `String` plus a copy into it.
        let mut out = crate::strbuf::StrBuf::new();
        for &v in parts {
            ctx_ref.heap.str_repr_into(v, &mut out);
        }
        ctx_ref.heap.alloc_str_dynamic(out.as_str())
    }
}

pub(crate) extern "C" fn jit_build_object_with_shape(
    ctx: *mut ExecCtx,
    closure: *const crate::closure::VmClosure,
    base: usize,
    start_reg: usize,
    shape_idx: usize,
) -> VmValue {
    unsafe {
        use crate::alloc_profile as prof;
        let on = prof::enabled();
        let t_all = if on { prof::read() } else { 0 };

        let ctx_ref = &mut *ctx;
        let closure_ref = &*closure;

        let det = prof::detail();
        let t_shape = if det { prof::read() } else { 0 };
        let resolved = closure_ref.proto.resolved_shape(shape_idx);
        if det {
            prof::record(prof::Seg::ShapeLookup, t_shape, prof::read());
        }
        let Some(shape) = resolved else {
            return VmValue::null();
        };
        let count = shape.property_names.len();

        let required = base + start_reg + count;
        if ctx_ref.stack.len() < required {
            ctx_ref.stack.resize(required, VmValue::null());
        }
        let out = crate::exec::collections::build_object_with_shape(
            &ctx_ref.stack,
            base + start_reg,
            shape,
            &mut ctx_ref.heap,
        );
        if on {
            prof::record(prof::Seg::HelperTotal, t_all, prof::read());
        }
        out
    }
}

pub(crate) extern "C" fn jit_build_record_with_shape(
    ctx: *mut ExecCtx,
    closure: *const crate::closure::VmClosure,
    base: usize,
    start_reg: usize,
    shape_idx: usize,
) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let closure_ref = &*closure;
        let Some(shape) = closure_ref.proto.resolved_shape(shape_idx) else {
            return VmValue::null();
        };
        let count = shape.property_names.len();

        let required = base + start_reg + count;
        if ctx_ref.stack.len() < required {
            ctx_ref.stack.resize(required, VmValue::null());
        }
        crate::exec::collections::build_record_with_shape(
            &ctx_ref.stack,
            base + start_reg,
            shape,
            &mut ctx_ref.heap,
        )
    }
}

pub(crate) extern "C" fn jit_range(
    ctx: *mut ExecCtx,
    start_val: VmValue,
    end_val: VmValue,
    flag: usize,
) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let mut temp = vec![start_val, end_val];
        match crate::exec::advanced::invoke_runtime_static(
            "__range__",
            &mut temp,
            &mut ctx_ref.heap,
            flag as u16,
        ) {
            Ok(v) => v,
            Err(e) => panic!("JIT range failed: {:?}", e),
        }
    }
}

pub(crate) extern "C" fn jit_wrap_spread_stub(ctx: *mut ExecCtx, val: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let extracted = ctx_ref.heap.extract(val);
        ctx_ref
            .heap
            .intern(varn_types::Value::Spread(Box::new(extracted)))
    }
}

pub(crate) extern "C" fn jit_build_object(
    ctx: *mut ExecCtx,
    closure: *const crate::closure::VmClosure,
    base: usize,
    ip_before: usize,
) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let closure_ref = &*closure;
        let code = &closure_ref.proto.chunk.code;

        let mut temp_ip = ip_before;
        let w1 = code[temp_ip];
        temp_ip += 1;
        let count = (w1 & 0xFF) as usize;
        let obj_nv = ctx_ref.heap.alloc_object();
        for _ in 0..count {
            let k_idx = code[temp_ip] as usize;
            temp_ip += 1;
            let w = code[temp_ip];
            temp_ip += 1;
            let val_reg = (w >> 8) as usize;
            let key_nv = closure_ref.constants[k_idx];
            let key = ctx_ref
                .heap
                .str_val(key_nv)
                .map(|s| s.to_string())
                .unwrap_or_else(|| {
                    closure_ref.proto.chunk.constants[k_idx]
                        .as_str()
                        .unwrap_or("")
                        .to_string()
                });
            let val = ctx_ref.stack[base + val_reg];
            if let Err(e) = crate::exec::props::set_property(obj_nv, &key, val, &mut ctx_ref.heap) {
                jit_propagate_error(ctx_ref, e);
            }
        }
        obj_nv
    }
}
