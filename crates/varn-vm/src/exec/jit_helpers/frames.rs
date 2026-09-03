//! The caller-to-callee frame handshake.
//!
//! `jit_prepare_call` stages the callee window and records the resume ip;
//! `jit_push_self_frame` and `jit_post_call` are the two halves the compiled
//! prologue and epilogue call into. The protocol is documented on
//! `ExecCtx::jit_frame_prepushed` — read that before touching any of this.

use super::calls::jit_guard_call_depth;
use super::construct::jit_propagate_error;
use crate::exec::ctx::ExecCtx;
use crate::exec::frame_ctrl::resolve_constructor_return;
use crate::value::VmValue;

pub(crate) extern "C" fn jit_prepare_call(
    ctx: *mut ExecCtx,
    callee: VmValue,
    callee_base: usize,
    arg_count: usize,
) -> *const crate::closure::VmClosure {
    unsafe {
        let ctx_ref = &mut *ctx;
        // Record the caller's post-call resume ip (written to `jit_resume_ip`
        // by the JIT call site). If the callee — or anything it calls — throws
        // and the exception is caught below this caller, the longjmp unwinds
        // this caller's native JIT frame; the interpreter then resumes it from
        // this ip instead of re-executing its JIT body from ip=0 (infinite
        // loop). Mirrors what `jit_call` does on the slow path.
        let resume_ip = ctx_ref.jit_resume_ip;
        let call_dest = ctx_ref.jit_call_dest as u16;
        if let Some(caller) = ctx_ref.frames.last_mut() {
            caller.ip = resume_ip;
        }
        if !callee.is_heap() {
            return std::ptr::null();
        }
        jit_guard_call_depth(ctx_ref);
        let heap_idx = callee.as_heap_idx();
        if let Some(closure) = ctx_ref.heap.get_closure(heap_idx) {
            if !closure.proto.is_async && !closure.proto.is_generator && !closure.proto.has_rest
                && closure.jit_fn().is_some() {
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
                    let mut frame = crate::frame::CallFrame::new(closure, callee_base);
                    // Return destination for a possible interpreted resume after
                    // an exception unwind (fast machine-return store is skipped
                    // in that case).
                    frame.return_reg = Some(call_dest);
                    ctx_ref.frames.push(frame);
                    return closure as *const crate::closure::VmClosure;
                }
        } else if let Some(crate::heap::HeapObj::Class(cls)) = ctx_ref.heap.get(heap_idx) {
            let cls = cls.clone();
            let oref = varn_types::value::ObjRef::instance(cls.clone());
            let instance_nv =
                VmValue::from_heap_idx(ctx_ref.heap.alloc(crate::heap::HeapObj::Object(oref)));
            ctx_ref.stack[callee_base] = instance_nv;
            if let Some(ctor) = cls.constructor() {
                match ctor {
                    varn_types::Value::VmValue(ref payload) => {
                        if let Some(wrapper) = payload
                            .as_any()
                            .downcast_ref::<crate::closure::VmClosurePayload>()
                        {
                            let closure = wrapper.0.clone();
                            if !closure.proto.is_async && !closure.proto.is_generator
                                && closure.jit_fn().is_some() {
                                    let required_cap =
                                        callee_base + closure.proto.register_count as usize + 32;
                                    let required_len =
                                        callee_base + closure.proto.register_count as usize;
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
                                    let ctor_closure_ptr =
                                        &*closure as *const crate::closure::VmClosure;
                                    let returning_frame_idx = ctx_ref.frames.len();
                                    ctx_ref
                                        .pending_constructors
                                        .push((returning_frame_idx, instance_nv));
                                    let mut frame =
                                        crate::frame::CallFrame::new(&closure, callee_base);
                                    frame.return_reg = Some(call_dest);
                                    ctx_ref.frames.push(frame);
                                    return ctor_closure_ptr;
                                }
                        }
                    }
                    varn_types::Value::NativeFn(ref b) => {
                        let (f, name) = **b;
                        ctx_ref.record_call_native(f, Some(name));
                        let result = if arg_count == 0 {
                            ctx_ref.invoke_native(f, &[])
                        } else {
                            if arg_count <= 8 {
                                let mut buf = [VmValue::null(); 8];
                                buf[..arg_count].copy_from_slice(
                                    &ctx_ref.stack[callee_base..(callee_base + arg_count)],
                                );
                                ctx_ref.invoke_native(f, &buf[..arg_count])
                            } else {
                                let vargs: Vec<VmValue> = (0..arg_count)
                                    .map(|i| ctx_ref.stack[callee_base + i])
                                    .collect();
                                ctx_ref.invoke_native(f, &vargs)
                            }
                        };
                        match result {
                            Ok(v) => {
                                let nv = if v.is_null() { instance_nv } else { v };
                                ctx_ref.jit_native_result = nv;
                                return std::ptr::dangling::<crate::closure::VmClosure>();
                            }
                            Err(msg) => {
                                let e = crate::error::RuntimeError::new(msg);
                                jit_propagate_error(ctx_ref, e);
                            }
                        }
                    }
                    _ => {}
                }
            } else {
                ctx_ref.jit_native_result = instance_nv;
                return std::ptr::dangling::<crate::closure::VmClosure>();
            }
        } else if let Some(crate::heap::HeapObj::NativeFn(f, name)) = ctx_ref.heap.get(heap_idx) {
            let f = *f;
            let name = *name;
            ctx_ref.record_call_native(f, Some(name));
            let base = callee_base;
            let result = if arg_count <= 1 {
                ctx_ref.invoke_native(f, &[])
            } else {
                let actual_count = arg_count - 1;
                if actual_count <= 8 {
                    let mut buf = [VmValue::null(); 8];
                    buf[..actual_count].copy_from_slice(
                        &ctx_ref.stack[(base + 1)..(base + 1 + actual_count)],
                    );
                    ctx_ref.invoke_native(f, &buf[..actual_count])
                } else {
                    let vargs: Vec<VmValue> = (1..=actual_count)
                        .map(|i| ctx_ref.stack[base + i])
                        .collect();
                    ctx_ref.invoke_native(f, &vargs)
                }
            };
            match result {
                Ok(v) => {
                    ctx_ref.jit_native_result = v;
                    return std::ptr::dangling::<crate::closure::VmClosure>();
                }
                Err(msg) => {
                    let e = crate::error::RuntimeError::new(msg);
                    jit_propagate_error(ctx_ref, e);
                }
            }
        }
        std::ptr::null()
    }
}

pub(crate) extern "C" fn jit_push_self_frame(ctx: *mut ExecCtx, callee_base: usize) {
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

pub(crate) extern "C" fn jit_post_call(
    ctx: *mut ExecCtx,
    callee_base: usize,
    val: VmValue,
) -> VmValue {
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

pub(crate) extern "C" fn jit_load_static_fn(
    ctx: *mut ExecCtx,
    closure: *const crate::closure::VmClosure,
    proto_idx: usize,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let closure_ref = &*closure;
        let proto = match closure_ref.proto.chunk.constants.get(proto_idx) {
            Some(varn_types::PoolEntry::Function(p)) => p,
            _ => panic!("LoadStaticFn: invalid function proto"),
        };

        let proto_ptr = std::rc::Rc::as_ptr(proto) as usize;
        if let Some(&(_, cached_val)) = ctx_ref.static_closures.get(&proto_ptr) {
            ctx_ref.jit_native_result = cached_val;
            return;
        }
        let constants = ctx_ref
            .proto_constants
            .entry(proto_ptr)
            .or_insert_with(|| {
                let resolved = std::rc::Rc::new(crate::exec::calls::resolve_constants(
                    proto,
                    &mut ctx_ref.heap,
                ));
                (proto.clone(), resolved)
            })
            .1
            .clone();
        let new_closure = crate::closure::VmClosure::with_upvalues(
            proto.clone(),
            vec![],
            constants,
            ctx_ref.settings,
        );
        let val = ctx_ref.heap.alloc_vm_closure(std::rc::Rc::new(new_closure));
        ctx_ref
            .static_closures
            .insert(proto_ptr, (proto.clone(), val));
        ctx_ref.jit_native_result = val;
    }
}
