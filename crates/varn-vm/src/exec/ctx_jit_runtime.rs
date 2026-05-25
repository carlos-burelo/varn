use crate::value::VmValue;

use super::ctx::ExecCtx;

pub extern "C" fn jit_negate(ctx: *mut ExecCtx, v: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        crate::exec::arith::negate(v, &mut ctx_ref.heap)
    }
}

pub extern "C" fn jit_logical_not(_ctx: *mut ExecCtx, v: VmValue) -> VmValue {
    crate::exec::compare::logical_not(v)
}

pub extern "C" fn jit_div(ctx: *mut ExecCtx, a: VmValue, b: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        match crate::exec::arith::div(a, b, &mut ctx_ref.heap) {
            Ok(v) => v,
            Err(e) => panic!("Runtime error in JIT div: {:?}", e),
        }
    }
}

pub extern "C" fn jit_modulo(ctx: *mut ExecCtx, a: VmValue, b: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        match crate::exec::arith::modulo(a, b, &mut ctx_ref.heap) {
            Ok(v) => v,
            Err(e) => panic!("Runtime error in JIT mod: {:?}", e),
        }
    }
}

pub extern "C" fn jit_pow(_ctx: *mut ExecCtx, a: VmValue, b: VmValue) -> VmValue {
    crate::exec::arith::pow(a, b)
}

pub extern "C" fn jit_get_index(
    ctx: *mut ExecCtx,
    args: *const varn_jit::JitGetIndexArgs,
) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let args = &*args;
        match crate::exec::collections::get_index(args.obj, args.key, &mut ctx_ref.heap) {
            Ok(v) => v,
            Err(e) => panic!("Runtime error in JIT get_index: {:?}", e),
        }
    }
}

pub extern "C" fn jit_set_index(ctx: *mut ExecCtx, args: *const varn_jit::JitSetIndexArgs) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let args = &*args;
        match crate::exec::collections::set_index(args.obj, args.key, args.val, &mut ctx_ref.heap) {
            Ok(()) => {}
            Err(e) => panic!("Runtime error in JIT set_index: {:?}", e),
        }
    }
}

pub extern "C" fn jit_typeof_val(ctx: *mut ExecCtx, v: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let s = crate::exec::advanced::typeof_val(v, &ctx_ref.heap);
        ctx_ref.heap.alloc_str(s)
    }
}

pub extern "C" fn jit_instanceof(ctx: *mut ExecCtx, a: VmValue, b: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &*ctx;
        let r = crate::exec::advanced::instanceof(a, b, &ctx_ref.heap);
        VmValue::from_bool(r)
    }
}

pub extern "C" fn jit_array_length(ctx: *mut ExecCtx, arr: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        match ctx_ref.exec_array_length(arr) {
            Ok(v) => v,
            Err(e) => panic!("Runtime error in JIT array_length: {:?}", e),
        }
    }
}

pub extern "C" fn jit_array_push(ctx: *mut ExecCtx, arr: VmValue, val: VmValue) {
    unsafe {
        let ctx_ref = &mut *ctx;
        match ctx_ref.exec_array_push(arr, val) {
            Ok(()) => {}
            Err(e) => panic!("Runtime error in JIT array_push: {:?}", e),
        }
    }
}

pub extern "C" fn jit_array_pop(ctx: *mut ExecCtx, arr: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        match ctx_ref.exec_array_pop(arr) {
            Ok(v) => v,
            Err(e) => panic!("Runtime error in JIT array_pop: {:?}", e),
        }
    }
}

pub extern "C" fn jit_array_extend(ctx: *mut ExecCtx, arr: VmValue, src: VmValue) {
    unsafe {
        let ctx_ref = &mut *ctx;
        match ctx_ref.exec_array_extend(arr, src) {
            Ok(()) => {}
            Err(e) => panic!("Runtime error in JIT array_extend: {:?}", e),
        }
    }
}

pub extern "C" fn jit_str_concat(ctx: *mut ExecCtx, a: VmValue, b: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let sa = ctx_ref.heap.str_repr(a);
        let sb = ctx_ref.heap.str_repr(b);
        let combined = format!("{sa}{sb}");
        ctx_ref.heap.alloc_str(&combined)
    }
}

pub extern "C" fn jit_str_slice(ctx: *mut ExecCtx, s: VmValue, idx: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        match ctx_ref.exec_str_slice(s, idx) {
            Ok(v) => v,
            Err(e) => panic!("Runtime error in JIT str_slice: {:?}", e),
        }
    }
}

pub extern "C" fn jit_str_length(ctx: *mut ExecCtx, v: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        match ctx_ref.exec_str_length(v) {
            Ok(len) => len,
            Err(e) => panic!("Runtime error in JIT str_length: {:?}", e),
        }
    }
}

pub extern "C" fn jit_bitand(_ctx: *mut ExecCtx, a: VmValue, b: VmValue) -> VmValue {
    crate::exec::arith::bit_and(a, b)
}

pub extern "C" fn jit_bitor(_ctx: *mut ExecCtx, a: VmValue, b: VmValue) -> VmValue {
    crate::exec::arith::bit_or(a, b)
}

pub extern "C" fn jit_bitxor(_ctx: *mut ExecCtx, a: VmValue, b: VmValue) -> VmValue {
    crate::exec::arith::bit_xor(a, b)
}

pub extern "C" fn jit_shl(_ctx: *mut ExecCtx, a: VmValue, b: VmValue) -> VmValue {
    crate::exec::arith::shl(a, b)
}

pub extern "C" fn jit_shr(_ctx: *mut ExecCtx, a: VmValue, b: VmValue) -> VmValue {
    crate::exec::arith::shr(a, b)
}

pub extern "C" fn jit_ushr(_ctx: *mut ExecCtx, a: VmValue, b: VmValue) -> VmValue {
    crate::exec::arith::ushr(a, b)
}

pub extern "C" fn jit_load_module(
    ctx: *mut ExecCtx,
    closure: *const crate::frame::VmClosure,
    const_idx: usize,
) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let closure_ref = &*closure;
        let spec_nv = closure_ref.constants[const_idx];
        let spec = match ctx_ref.heap.str_val(spec_nv) {
            Some(s) => s,
            None => return VmValue::null(),
        };
        match ctx_ref.load_module(&spec) {
            Ok(v) => v,
            Err(e) => panic!("JIT load_module failed: {:?}", e),
        }
    }
}

pub extern "C" fn jit_load_module_slot(
    ctx: *mut ExecCtx,
    module_val: VmValue,
    slot_idx: usize,
) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        if !module_val.is_heap() {
            return VmValue::null();
        }
        if let Some(crate::heap::HeapObj::Module(m)) = ctx_ref.heap.get(module_val.as_heap_idx()) {
            m.get_slot(slot_idx).unwrap_or(VmValue::null())
        } else {
            VmValue::null()
        }
    }
}

pub extern "C" fn jit_build_object_with_shape(
    ctx: *mut ExecCtx,
    closure: *const crate::frame::VmClosure,
    start_reg: usize,
    shape_idx: usize,
) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let closure_ref = &*closure;
        let keys = match closure_ref.proto.chunk.constants.get(shape_idx) {
            Some(varn_types::chunk::PoolEntry::Shape(k)) => k.clone(),
            _ => return VmValue::null(),
        };
        let count = keys.len();
        let frame_idx = ctx_ref.frames.len() - 1;
        let base = ctx_ref.frames[frame_idx].base;

        let mut inner = varn_types::RuntimeObject::new();
        for (i, key) in keys.iter().enumerate() {
            let val_nv = ctx_ref.stack[base + start_reg + i];
            if val_nv.is_heap() {
                if let Some(crate::heap::HeapObj::VmClosure(nc)) =
                    ctx_ref.heap.get(val_nv.as_heap_idx())
                {
                    let nc = nc.clone();
                    for uv in &nc.upvalues {
                        uv.close(&ctx_ref.stack);
                    }
                }
            }
            inner.insert(key.clone(), val_nv);
        }
        let oref = varn_types::value::ObjRef::new(varn_types::ObjData::from_inner(inner));
        VmValue::from_heap_idx(ctx_ref.heap.alloc(crate::heap::HeapObj::Object(oref)))
    }
}

pub extern "C" fn jit_range(
    ctx: *mut ExecCtx,
    start_reg: usize,
    end_reg: usize,
    flag: usize,
) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let frame_idx = ctx_ref.frames.len() - 1;
        let base = ctx_ref.frames[frame_idx].base;
        let start_val = ctx_ref.stack[base + start_reg];
        let end_val = ctx_ref.stack[base + end_reg];

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
