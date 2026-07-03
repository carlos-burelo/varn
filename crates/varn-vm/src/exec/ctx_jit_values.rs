use super::ctx::ExecCtx;
use crate::value::VmValue;

#[inline(always)]
pub(crate) unsafe fn jit_propagate_error(ctx: &mut ExecCtx, e: crate::error::RuntimeError) -> ! {
    let handler = ctx.try_handlers.pop();
    ctx.jit_panic_exception_handler = handler;
    ctx.jit_panic_exception_error = Some(e.thrown.unwrap_or(VmValue::null()));
    ctx.jit_panic_exception_err_obj = Some(e);
    let buf = ctx.jit_jmp_buf;
    if !buf.is_null() {
        crate::exec::ctx::my_longjmp(buf, 1);
    }
    panic!("JIT error: no jump buffer");
}

#[inline(always)]
fn resolve_constructor_return(
    ctx: &mut ExecCtx,
    returning_frame_idx: usize,
    val: VmValue,
) -> VmValue {
    let ctor_pos = if !ctx.pending_constructors.is_empty() {
        ctx.pending_constructors
            .iter()
            .rposition(|(idx, _)| *idx == returning_frame_idx)
    } else {
        None
    };

    if let Some(pos) = ctor_pos {
        let (_, instance_nv) = ctx.pending_constructors.remove(pos);
        if val.is_null() {
            instance_nv
        } else {
            val
        }
    } else {
        val
    }
}

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

        let val = closure_ref.upvalues[uv_idx].read(&ctx_ref.stack);
        val
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

        for ___ in 0..uv_count {
            let uv_desc = code[ip];

            ip += 1;

            let is_local = (uv_desc >> 8) != 0;

            let index = (uv_desc & 0xFF) as usize;

            if is_local {
                let slot = base + index;

                let captured = ctx_ref.capture_upvalue(slot);

                upvalues.push(captured);
            } else {
                let captured = closure_ref.upvalues[index].clone();

                upvalues.push(captured);
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

/// Maximum VM call depth before a graceful error is raised. Kept in sync with
/// the interpreter guard (`calls.rs`). JIT'd calls recurse on the native Rust
/// stack (the JIT invokes the callee's `jit_entry` directly), so without this
/// guard deep recursion aborts the host process instead of producing a
/// catchable runtime error.
const MAX_CALL_DEPTH: usize = 10000;

#[inline(always)]
unsafe fn jit_guard_call_depth(ctx: &mut ExecCtx) {
    if ctx.frames.len() >= MAX_CALL_DEPTH {
        let e = crate::error::RuntimeError::new(format!(
            "stack overflow: call depth exceeded {MAX_CALL_DEPTH}"
        ));
        jit_propagate_error(ctx, e);
    }
}

pub extern "C" fn jit_call(ctx: *mut ExecCtx, args: *const varn_jit::JitCallArgs) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;

        let args = &*args;

        jit_guard_call_depth(ctx_ref);

        let caller_depth = ctx_ref.frames.len();

        let frame_idx = caller_depth - 1;

        let base = ctx_ref.frames[frame_idx].base;

        ctx_ref.frames[frame_idx].ip = args.ip;

        if args.callee.is_heap() {
            let heap_obj = ctx_ref.heap.get(args.callee.as_heap_idx());

            if let Some(crate::heap::HeapObj::VmClosure(closure)) = heap_obj {
                let is_eligible = !closure.proto.is_async && !closure.proto.is_generator;

                if let Some(jit_fn) = closure.jit_entry.filter(|_| is_eligible) {
                    let callee_base = base + args.arg_start;

                    let required = callee_base + closure.proto.register_count as usize + 32;

                    if ctx_ref.stack.len() < required {
                        ctx_ref.stack.resize(required, VmValue::null());
                    }

                    ctx_ref
                        .frames
                        .push(crate::frame::CallFrame::new(&**closure, callee_base));

                    let res = (jit_fn)(
                        ctx_ref.stack.as_mut_ptr() as *mut std::ffi::c_void,
                        &**closure as *const crate::frame::VmClosure as *const std::ffi::c_void,
                        callee_base,
                        ctx_ref as *mut ExecCtx as *mut std::ffi::c_void,
                    );

                    let returning_frame_idx = ctx_ref.frames.len() - 1;

                    ctx_ref.frames.pop();

                    ctx_ref.close_upvalues_above(callee_base);

                    let final_val = resolve_constructor_return(ctx_ref, returning_frame_idx, res);

                    ctx_ref.stack[base + args.dest] = final_val;

                    ctx_ref.record_call_vm_fast();

                    return final_val;
                }
            } else if let Some(crate::heap::HeapObj::NativeFn(_, f)) = heap_obj {
                let f = *f;
                ctx_ref.record_call_native();
                let arg_base = base + args.arg_start;

                let result = if args.arg_count <= 1 {
                    ctx_ref.invoke_native(f, &[])
                } else {
                    let actual_count = args.arg_count - 1;
                    if actual_count <= 8 {
                        let mut buf = [VmValue::null(); 8];
                        for i in 0..actual_count {
                            buf[i] = ctx_ref.stack[arg_base + 1 + i];
                        }
                        ctx_ref.invoke_native(f, &buf[..actual_count])
                    } else {
                        let vargs: Vec<VmValue> = (1..=actual_count)
                            .map(|i| ctx_ref.stack[arg_base + i])
                            .collect();
                        ctx_ref.invoke_native(f, &vargs)
                    }
                };
                let v = match result {
                    Ok(v) => v,
                    Err(msg) => {
                        let e = crate::error::RuntimeError::new(msg);
                        jit_propagate_error(ctx_ref, e);
                    }
                };
                ctx_ref.stack[base + args.dest] = v;
                return v;
            }
        }

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
                if let Err(e) = ctx_ref.run_until_inner(caller_depth) {
                    jit_propagate_error(ctx_ref, e);
                }
            }
            Ok(false) => {}
            Err(e) => jit_propagate_error(ctx_ref, e),
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
                if let Err(e) = ctx_ref.run_until_inner(caller_depth) {
                    jit_propagate_error(ctx_ref, e);
                }
            }
            Ok(false) => {}
            Err(e) => jit_propagate_error(ctx_ref, e),
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
                if let Err(e) = ctx_ref.run_until_inner(caller_depth) {
                    jit_propagate_error(ctx_ref, e);
                }
            }
            Ok(false) => {}
            Err(e) => jit_propagate_error(ctx_ref, e),
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
                if let Err(e) = ctx_ref.run_until_inner(caller_depth) {
                    jit_propagate_error(ctx_ref, e);
                }
            }
            Ok(false) => {}
            Err(e) => jit_propagate_error(ctx_ref, e),
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

pub extern "C" fn jit_invoke_virtual(
    ctx: *mut ExecCtx,
    closure: *const crate::frame::VmClosure,
    args: *const varn_jit::JitInvokeVirtualArgs,
) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let closure_ref = &*closure;
        let args = &*args;
        let caller_depth = ctx_ref.frames.len();
        let frame_idx = caller_depth - 1;
        let base = ctx_ref.frames[frame_idx].base;

        ctx_ref.frames[frame_idx].ip = args.ip;

        let method_name_nv = closure_ref.constants[args.name_idx];
        let method_name = ctx_ref
            .heap
            .str_val(method_name_nv)
            .expect("InvokeVirtual: not a string const");

        let method_nv =
            crate::exec::props::get_property(args.this_val, &method_name, &mut ctx_ref.heap)
                .unwrap();

        if method_nv.is_heap() {
            let heap_obj = ctx_ref.heap.get(method_nv.as_heap_idx());
            if let Some(crate::heap::HeapObj::VmClosure(closure)) = heap_obj {
                let is_eligible = !closure.proto.is_async && !closure.proto.is_generator;
                if let Some(jit_fn) = closure.jit_entry.filter(|_| is_eligible) {
                    let callee_base = base + args.arg_start;
                    let required = callee_base + closure.proto.register_count as usize + 32;
                    if ctx_ref.stack.len() < required {
                        ctx_ref.stack.resize(required, VmValue::null());
                    }
                    ctx_ref
                        .frames
                        .push(crate::frame::CallFrame::new(&**closure, callee_base));
                    let res = (jit_fn)(
                        ctx_ref.stack.as_mut_ptr() as *mut std::ffi::c_void,
                        &**closure as *const crate::frame::VmClosure as *const std::ffi::c_void,
                        callee_base,
                        ctx_ref as *mut ExecCtx as *mut std::ffi::c_void,
                    );
                    let returning_frame_idx = ctx_ref.frames.len() - 1;
                    ctx_ref.frames.pop();
                    ctx_ref.close_upvalues_above(callee_base);
                    let final_val = resolve_constructor_return(ctx_ref, returning_frame_idx, res);
                    ctx_ref.stack[base + args.dest] = final_val;
                    ctx_ref.record_call_vm_fast();
                    return final_val;
                }
            }
        }

        let jumped = ctx_ref.exec_call_reg(
            method_nv,
            base,
            args.arg_start,
            args.arg_count,
            args.dest,
            frame_idx,
        );

        match jumped {
            Ok(true) => {
                if let Err(e) = ctx_ref.run_until_inner(caller_depth) {
                    jit_propagate_error(ctx_ref, e);
                }
            }
            Ok(false) => {}
            Err(e) => jit_propagate_error(ctx_ref, e),
        }

        ctx_ref.stack[base + args.dest]
    }
}

pub extern "C" fn jit_get_property_ic_fast(
    ctx: *mut ExecCtx,
    closure: *const crate::frame::VmClosure,
    obj: VmValue,
    cs_idx: usize,
) -> VmValue {
    unsafe {
        let ctx_ref = &*ctx;
        let closure_ref = &*closure;
        if cs_idx < closure_ref.ic_cache_len() && obj.is_heap() {
            if let Some(crate::heap::HeapObj::Object(o)) = ctx_ref.heap.get(obj.as_heap_idx()) {
                let guard = &*o.0.as_ptr();
                let slot_cache = &*closure_ref.ic_cache.as_ptr();
                let poly_slot = &slot_cache[cs_idx];
                for entry in &poly_slot.entries {
                    if entry.id != 0 && entry.is_class == 1 {
                        if guard.inner.shape.id == entry.id {
                            let slot = entry.slot as usize;
                            if slot < guard.inner.values.len() {
                                return guard.inner.values[slot];
                            }
                        }
                    }
                }
            }

            if let Some(cls) = crate::exec::props::get_class(obj, &ctx_ref.heap) {
                let slot_cache = &*closure_ref.ic_cache.as_ptr();
                let poly_slot = &slot_cache[cs_idx];
                for entry in &poly_slot.entries {
                    if entry.id == 0 {
                        continue;
                    }
                    if entry.is_class == 8 && cls.id == entry.id {
                        if let Some(crate::heap::HeapObj::Array(arr)) =
                            ctx_ref.heap.get(obj.as_heap_idx())
                        {
                            let len = arr.len();
                            return VmValue::from_int(len as i64);
                        }
                    } else if entry.is_class == 9 && cls.id == entry.id {
                        if let Some(crate::heap::HeapObj::Str(s)) =
                            ctx_ref.heap.get(obj.as_heap_idx())
                        {
                            let len = s.len();
                            return VmValue::from_int(len as i64);
                        }
                    }
                }
            }
        }
        VmValue(0x7FF8_0000_0000_0000)
    }
}

pub extern "C" fn jit_get_property_maybe_ic_fast(
    ctx: *mut ExecCtx,
    closure: *const crate::frame::VmClosure,
    obj: VmValue,
    cs_idx: usize,
) -> VmValue {
    jit_get_property_ic_fast(ctx, closure, obj, cs_idx)
}

pub extern "C" fn jit_prepare_call(
    ctx: *mut ExecCtx,
    callee: VmValue,
    callee_base: usize,
) -> *const crate::frame::VmClosure {
    unsafe {
        let ctx_ref = &mut *ctx;
        if !callee.is_heap() {
            return std::ptr::null();
        }
        jit_guard_call_depth(ctx_ref);
        if let Some(closure) = ctx_ref.heap.get_closure(callee.as_heap_idx()) {
            if !closure.proto.is_async && !closure.proto.is_generator {
                if closure.jit_entry.is_some() {
                    let required_cap = callee_base + closure.proto.register_count as usize + 32;
                    let required_len = callee_base + closure.proto.register_count as usize;
                    let stack_len = ctx_ref.stack.len();
                    if ctx_ref.stack.capacity() < required_cap {
                        ctx_ref.stack.reserve(required_cap - stack_len);
                    }
                    if stack_len < required_len {
                        ctx_ref.stack.set_len(required_len);
                        let ptr = ctx_ref.stack.as_mut_ptr();
                        for i in stack_len..required_len {
                            std::ptr::write(ptr.add(i), VmValue::null());
                        }
                    }
                    ctx_ref
                        .frames
                        .push(crate::frame::CallFrame::new(closure, callee_base));
                    return closure as *const crate::frame::VmClosure;
                }
            }
        }
        std::ptr::null()
    }
}

pub extern "C" fn jit_push_self_frame(ctx: *mut ExecCtx, callee_base: usize) {
    unsafe {
        let ctx_ref = &mut *ctx;
        jit_guard_call_depth(ctx_ref);
        let current_closure = ctx_ref.frames.last().unwrap().closure_ptr;
        let closure = &*current_closure;

        let required_cap = callee_base + closure.proto.register_count as usize + 32;
        let required_len = callee_base + closure.proto.register_count as usize;
        let stack_len = ctx_ref.stack.len();
        if ctx_ref.stack.capacity() < required_cap {
            ctx_ref.stack.reserve(required_cap - stack_len);
        }
        if stack_len < required_len {
            ctx_ref.stack.set_len(required_len);
            let ptr = ctx_ref.stack.as_mut_ptr();
            for i in stack_len..required_len {
                std::ptr::write(ptr.add(i), VmValue::null());
            }
        }
        ctx_ref
            .frames
            .push(crate::frame::CallFrame::new(closure, callee_base));
    }
}

pub extern "C" fn jit_post_call(ctx: *mut ExecCtx, callee_base: usize, val: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let returning_frame_idx = ctx_ref.frames.len() - 1;
        ctx_ref.frames.pop();
        if !ctx_ref.open_upvalues.is_empty() {
            ctx_ref.close_upvalues_above(callee_base);
        }
        ctx_ref.record_call_vm_fast();
        if !ctx_ref.pending_constructors.is_empty() {
            resolve_constructor_return(ctx_ref, returning_frame_idx, val)
        } else {
            val
        }
    }
}

pub extern "C" fn jit_load_static_fn(
    ctx: *mut ExecCtx,
    closure: *const crate::frame::VmClosure,
    proto_idx: usize,
) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let closure_ref = &*closure;
        let proto = match closure_ref.proto.chunk.constants.get(proto_idx) {
            Some(varn_types::PoolEntry::Function(p)) => p,
            _ => panic!("LoadStaticFn: invalid function proto"),
        };

        let proto_ptr = std::rc::Rc::as_ptr(proto) as usize;
        if let Some(&cached_val) = ctx_ref.static_closures.get(&proto_ptr) {
            return cached_val;
        }
        let constants = ctx_ref
            .proto_constants
            .entry(proto_ptr)
            .or_insert_with(|| {
                std::rc::Rc::new(crate::exec::calls::resolve_constants(
                    proto,
                    &mut ctx_ref.heap,
                ))
            })
            .clone();
        let new_closure = crate::frame::VmClosure::with_upvalues(proto.clone(), vec![], constants);
        let val = ctx_ref.heap.alloc_vm_closure(std::rc::Rc::new(new_closure));
        ctx_ref.static_closures.insert(proto_ptr, val);
        val
    }
}

pub extern "C" fn jit_dispatch_intrinsic(
    ctx: *mut ExecCtx,
    wire_byte: usize,
    args_start: usize,
    arg_count: usize,
) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let args = &ctx_ref.stack[args_start..args_start + arg_count];
        match crate::exec::intrinsics::dispatch(wire_byte as u8, args, &mut ctx_ref.heap) {
            Ok(v) => v,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

pub extern "C" fn jit_ensure_stack_capacity(ctx: *mut ExecCtx, required_len: usize) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let required_cap = required_len + 32;
        let stack_len = ctx_ref.stack.len();
        if ctx_ref.stack.capacity() < required_cap {
            ctx_ref.stack.reserve(required_cap - stack_len);
        }
        if stack_len < required_len {
            ctx_ref.stack.set_len(required_len);
            let ptr = ctx_ref.stack.as_mut_ptr();
            for i in stack_len..required_len {
                std::ptr::write(ptr.add(i), VmValue::null());
            }
        }
    }
}

pub extern "C" fn jit_is_native_fn(ctx: *mut ExecCtx, callee: VmValue) -> usize {
    unsafe {
        let ctx_ref = &*ctx;
        if callee.is_heap() {
            if let Some(crate::heap::HeapObj::NativeFn(..)) = ctx_ref.heap.get(callee.as_heap_idx())
            {
                return 1;
            }
        }
        0
    }
}

pub extern "C" fn jit_call_native_fast(
    ctx: *mut ExecCtx,
    callee: VmValue,
    arg_start: usize,
    arg_count: usize,
) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let caller_depth = ctx_ref.frames.len();
        let frame_idx = caller_depth - 1;
        let base = ctx_ref.frames[frame_idx].base;

        if callee.is_heap() {
            let heap_obj = ctx_ref.heap.get(callee.as_heap_idx());
            if let Some(crate::heap::HeapObj::NativeFn(_name, f)) = heap_obj {
                let f = *f;
                ctx_ref.record_call_native();
                let arg_base = base + arg_start;

                let result = if arg_count <= 1 {
                    ctx_ref.invoke_native(f, &[])
                } else {
                    let actual_count = arg_count - 1;
                    if actual_count <= 8 {
                        let mut buf = [VmValue::null(); 8];
                        for i in 0..actual_count {
                            buf[i] = ctx_ref.stack[arg_base + 1 + i];
                        }
                        ctx_ref.invoke_native(f, &buf[..actual_count])
                    } else {
                        let vargs: Vec<VmValue> = (1..=actual_count)
                            .map(|i| ctx_ref.stack[arg_base + i])
                            .collect();
                        ctx_ref.invoke_native(f, &vargs)
                    }
                };
                match result {
                    Ok(v) => return v,
                    Err(msg) => {
                        let e = crate::error::RuntimeError::new(msg);
                        jit_propagate_error(ctx_ref, e);
                    }
                }
            }
        }
        panic!("jit_call_native_fast called with non-native callee");
    }
}

/// JIT helper for `CallNativeOp`: resolve the stable op-id to its native fn and
/// invoke it. The stack slice `[receiver, args...]` is already laid out
/// contiguously at `args_start` (absolute), which is exactly the layout the
/// macro-generated wrapper expects — so this mirrors the interpreter's
/// `CallNativeOp` arm (and its `call_native_with_receiver` path).
pub extern "C" fn jit_call_native_op(
    ctx: *mut ExecCtx,
    op_id: u64,
    args_start: usize,
    total: usize,
) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let f = match varn_builtins::native_op_fn(op_id) {
            Some(f) => f,
            None => jit_propagate_error(
                ctx_ref,
                crate::error::RuntimeError::new(format!("CallNativeOp: unknown op-id {op_id}")),
            ),
        };
        call_native_from_stack(ctx_ref, f, args_start, total)
    }
}

/// `CallNativeOp` with the target already resolved at JIT-compile time —
/// no per-call op-id hash lookup.
pub extern "C" fn jit_call_native_fnptr(
    ctx: *mut ExecCtx,
    fn_addr: usize,
    args_start: usize,
    total: usize,
) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let f: varn_types::NativeFn = std::mem::transmute(fn_addr);
        call_native_from_stack(ctx_ref, f, args_start, total)
    }
}

unsafe fn call_native_from_stack(
    ctx_ref: &mut ExecCtx,
    f: varn_types::NativeFn,
    args_start: usize,
    total: usize,
) -> VmValue {
    ctx_ref.record_call_native();
    let result = if total <= 16 {
        let mut buf = [VmValue::null(); 16];
        std::ptr::copy_nonoverlapping(
            ctx_ref.stack.as_ptr().add(args_start),
            buf.as_mut_ptr(),
            total,
        );
        ctx_ref.invoke_native(f, &buf[..total])
    } else {
        let v: Vec<VmValue> = (0..total).map(|i| ctx_ref.stack[args_start + i]).collect();
        ctx_ref.invoke_native(f, &v)
    };
    match result {
        Ok(v) => v,
        Err(msg) => jit_propagate_error(ctx_ref, crate::error::RuntimeError::new(msg)),
    }
}
