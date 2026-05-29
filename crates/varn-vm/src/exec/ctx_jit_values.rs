use super::ctx::ExecCtx;
use crate::value::VmValue;

pub extern "C" fn jit_load_const(closure: *const crate::frame::VmClosure, idx: usize) -> VmValue {
    unsafe {
        let closure_ref = &*closure;
        closure_ref.constants[idx]
    }
}

pub extern "C" fn jit_load_global_idx(ctx: *mut ExecCtx, idx: usize) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        ctx_ref.globals.get_by_index(idx).unwrap_or(VmValue::null())
    }
}

pub extern "C" fn jit_store_global_idx(ctx: *mut ExecCtx, idx: usize, val: VmValue) {
    unsafe {
        let ctx_ref = &mut *ctx;
        ctx_ref.globals.set_by_index(idx, val);
    }
}

pub extern "C" fn jit_define_global_idx(ctx: *mut ExecCtx, idx: usize, val: VmValue) {
    unsafe {
        let ctx_ref = &mut *ctx;
        ctx_ref.globals.set_by_index(idx, val);
    }
}

pub extern "C" fn jit_eq(ctx: *mut ExecCtx, a: VmValue, b: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let res = crate::exec::compare::eq(a, b, &ctx_ref.heap);
        VmValue::from_bool(res)
    }
}

pub extern "C" fn jit_neq(ctx: *mut ExecCtx, a: VmValue, b: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let res = crate::exec::compare::neq(a, b, &ctx_ref.heap);
        VmValue::from_bool(res)
    }
}

pub extern "C" fn jit_lt(ctx: *mut ExecCtx, a: VmValue, b: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let res = crate::exec::compare::lt_heap(a, b, &ctx_ref.heap);
        VmValue::from_bool(res)
    }
}

pub extern "C" fn jit_lte(ctx: *mut ExecCtx, a: VmValue, b: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let res = crate::exec::compare::lte_heap(a, b, &ctx_ref.heap);
        VmValue::from_bool(res)
    }
}

pub extern "C" fn jit_gt(ctx: *mut ExecCtx, a: VmValue, b: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let res = crate::exec::compare::gt_heap(a, b, &ctx_ref.heap);
        VmValue::from_bool(res)
    }
}

pub extern "C" fn jit_gte(ctx: *mut ExecCtx, a: VmValue, b: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let res = crate::exec::compare::gte_heap(a, b, &ctx_ref.heap);
        VmValue::from_bool(res)
    }
}

pub extern "C" fn jit_add(ctx: *mut ExecCtx, a: VmValue, b: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        crate::exec::arith::add(a, b, &mut ctx_ref.heap).unwrap()
    }
}

pub extern "C" fn jit_sub(ctx: *mut ExecCtx, a: VmValue, b: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        crate::exec::arith::sub(a, b, &mut ctx_ref.heap).unwrap()
    }
}

pub extern "C" fn jit_mul(ctx: *mut ExecCtx, a: VmValue, b: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        crate::exec::arith::mul(a, b, &mut ctx_ref.heap).unwrap()
    }
}

pub extern "C" fn jit_to_string(ctx: *mut ExecCtx, v: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let s = ctx_ref.heap.str_repr(v);
        ctx_ref.heap.alloc_str(&s)
    }
}

pub extern "C" fn jit_load_global(
    ctx: *mut ExecCtx,
    closure: *const crate::frame::VmClosure,
    name_idx: usize,
) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let closure_ref = &*closure;
        let name_nv = closure_ref.constants[name_idx];
        let name = ctx_ref.heap.str_val(name_nv).unwrap();
        ctx_ref
            .globals
            .get_by_name(&name)
            .unwrap_or(VmValue::null())
    }
}

pub extern "C" fn jit_load_upvalue(
    ctx: *mut ExecCtx,
    closure: *const crate::frame::VmClosure,
    uv_idx: usize,
) -> VmValue {
    unsafe {
        let ctx_ref = &*ctx;
        let closure_ref = &*closure;
        closure_ref.upvalues[uv_idx].read(&ctx_ref.stack)
    }
}

pub extern "C" fn jit_store_upvalue(
    ctx: *mut ExecCtx,
    closure: *const crate::frame::VmClosure,
    uv_idx: usize,
    val: VmValue,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let closure_ref = &*closure;
        closure_ref.upvalues[uv_idx].write(val, &mut ctx_ref.stack);
    }
}

pub extern "C" fn jit_make_closure(
    ctx: *mut ExecCtx,
    closure: *const crate::frame::VmClosure,
    ip_offset: usize,
    base: usize,
) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let closure_ref = &*closure;
        let code = &closure_ref.proto.chunk.code;
        let mut ip = ip_offset;

        let w1 = code[ip];
        ip += 1;
        let proto_idx = code[ip] as usize;
        ip += 1;

        let uv_count = (w1 & 0xFF) as usize;

        let proto = match closure_ref.proto.chunk.constants.get(proto_idx) {
            Some(varn_types::PoolEntry::Function(p)) => p.clone(),
            _ => panic!("MakeClosure: invalid function proto"),
        };

        let mut upvalues = Vec::with_capacity(uv_count);
        for _ in 0..uv_count {
            let uv_desc = code[ip];
            ip += 1;
            let is_local = (uv_desc >> 8) != 0;
            let index = (uv_desc & 0xFF) as usize;
            if is_local {
                upvalues.push(ctx_ref.capture_upvalue(base + index));
            } else {
                upvalues.push(closure_ref.upvalues[index].clone());
            }
        }

        let proto_ptr = std::rc::Rc::as_ptr(&proto) as usize;
        let constants = ctx_ref
            .proto_constants
            .entry(proto_ptr)
            .or_insert_with(|| {
                std::rc::Rc::new(crate::exec::calls::resolve_constants(
                    &proto,
                    &mut ctx_ref.heap,
                ))
            })
            .clone();

        let new_closure = crate::frame::VmClosure::with_upvalues(proto, upvalues, constants);

        ctx_ref.heap.alloc_vm_closure(std::rc::Rc::new(new_closure))
    }
}

pub extern "C" fn jit_call(ctx: *mut ExecCtx, args: *const varn_jit::JitCallArgs) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let args = &*args;
        let caller_depth = ctx_ref.frames.len();
        let frame_idx = caller_depth - 1;
        let base = ctx_ref.frames[frame_idx].base;

        ctx_ref.frames[frame_idx].ip = args.ip;

        // Fast path: plain NativeFn — skips exec_call_reg dispatch + BoundMethod check
        if args.callee.is_heap() {
            if let Some(crate::heap::HeapObj::NativeFn(_name, f)) =
                ctx_ref.heap.get(args.callee.as_heap_idx())
            {
                let f = *f;
                ctx_ref.record_call_native();
                let arg_base = base + args.arg_start;
                // stack layout: [callee, arg1, arg2, ...]; actual args start at +1
                let result = if args.arg_count <= 1 {
                    (f)(ctx_ref as &mut dyn varn_types::NativeCtx, &[])
                } else {
                    let actual_count = args.arg_count - 1;
                    if actual_count <= 8 {
                        let mut buf = [VmValue::null(); 8];
                        for i in 0..actual_count {
                            buf[i] = ctx_ref.stack[arg_base + 1 + i];
                        }
                        (f)(
                            ctx_ref as &mut dyn varn_types::NativeCtx,
                            &buf[..actual_count],
                        )
                    } else {
                        let vargs: Vec<VmValue> = (1..=actual_count)
                            .map(|i| ctx_ref.stack[arg_base + i])
                            .collect();
                        (f)(ctx_ref as &mut dyn varn_types::NativeCtx, &vargs)
                    }
                };
                let v = match result {
                    Ok(v) => v,
                    Err(e) => panic!("Runtime error in JIT native call: {:?}", e),
                };
                ctx_ref.stack[base + args.dest] = v;
                return v;
            }
        }

        // General dispatch
        let res = ctx_ref.exec_call_reg(
            args.callee,
            base,
            args.arg_start,
            args.arg_count,
            args.dest,
            frame_idx,
        );

        match res {
            Ok(true) => {
                ctx_ref.run_until_inner(caller_depth).unwrap();
            }
            Ok(false) => {}
            Err(e) => {
                panic!("Runtime error in JIT call: {:?}", e);
            }
        }

        ctx_ref.stack[base + args.dest]
    }
}

pub extern "C" fn jit_call_method(
    ctx: *mut ExecCtx,
    closure: *const crate::frame::VmClosure,
    args: *const varn_jit::JitCallMethodArgs,
) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let closure_ref = &*closure;
        let args = &*args;
        let caller_depth = ctx_ref.frames.len();
        let frame_idx = caller_depth - 1;
        let base = ctx_ref.frames[frame_idx].base;

        ctx_ref.frames[frame_idx].ip = args.ip;

        let res = ctx_ref.exec_call_method_reg(
            args.this_val,
            base,
            args.name_idx,
            args.cs,
            args.arg_start,
            args.arg_count,
            args.dest,
            frame_idx,
            closure_ref,
        );

        match res {
            Ok(true) => {
                ctx_ref.run_until_inner(caller_depth).unwrap();
            }
            Ok(false) => {}
            Err(e) => {
                panic!("Runtime error in JIT call_method: {:?}", e);
            }
        }

        ctx_ref.stack[base + args.dest]
    }
}

pub extern "C" fn jit_get_property(
    ctx: *mut ExecCtx,
    closure: *const crate::frame::VmClosure,
    args: *const varn_jit::JitGetPropertyArgs,
) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let closure_ref = &*closure;
        let args = &*args;
        let caller_depth = ctx_ref.frames.len();
        let frame_idx = caller_depth - 1;
        let base = ctx_ref.frames[frame_idx].base;

        ctx_ref.frames[frame_idx].ip = args.ip;

        let res = ctx_ref.exec_get_property_reg(
            args.obj,
            args.name_idx,
            args.cs_idx,
            args.dest,
            base,
            frame_idx,
            closure_ref,
        );

        match res {
            Ok(true) => {
                ctx_ref.run_until_inner(caller_depth).unwrap();
            }
            Ok(false) => {}
            Err(e) => {
                panic!("Runtime error in JIT get_property: {:?}", e);
            }
        }

        ctx_ref.stack[base + args.dest]
    }
}

pub extern "C" fn jit_set_property(
    ctx: *mut ExecCtx,
    closure: *const crate::frame::VmClosure,
    args: *const varn_jit::JitSetPropertyArgs,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let closure_ref = &*closure;
        let args = &*args;
        let caller_depth = ctx_ref.frames.len();
        let frame_idx = caller_depth - 1;
        let base = ctx_ref.frames[frame_idx].base;

        ctx_ref.frames[frame_idx].ip = args.ip;

        let res = ctx_ref.exec_set_property_reg(
            args.obj,
            args.val,
            args.name_idx,
            args.cs_idx,
            base,
            frame_idx,
            closure_ref,
        );

        match res {
            Ok(true) => {
                ctx_ref.run_until_inner(caller_depth).unwrap();
            }
            Ok(false) => {}
            Err(e) => {
                panic!("Runtime error in JIT set_property: {:?}", e);
            }
        }
    }
}

pub extern "C" fn jit_build_array(ctx: *mut ExecCtx, start_reg: usize, count: usize) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let frame_idx = ctx_ref.frames.len() - 1;
        let base = ctx_ref.frames[frame_idx].base;
        let mut elems = Vec::with_capacity(count);
        for i in 0..count {
            let nv = ctx_ref.stack[base + start_reg + i];
            elems.push(nv);
        }
        ctx_ref.heap.alloc_array_vm(elems)
    }
}

pub extern "C" fn jit_build_str(
    ctx: *mut ExecCtx,
    parts_ptr: *const VmValue,
    count: usize,
) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let parts = std::slice::from_raw_parts(parts_ptr, count);
        let mut total_len = 0;
        let mut string_parts = Vec::with_capacity(count);
        for &v in parts {
            let s = ctx_ref.heap.str_repr(v);
            total_len += s.len();
            string_parts.push(s);
        }
        let mut combined = String::with_capacity(total_len);
        for s in &string_parts {
            combined.push_str(s);
        }
        ctx_ref.heap.alloc_str(&combined)
    }
}
