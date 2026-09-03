//! Calling VM code from compiled code.
//!
//! `jit_call` and the method-call variants all converge on the same problem:
//! the callee may be compiled, interpreted, native or bound, and the frame
//! protocol has to come out identical either way.

use super::construct::{jit_construct_fast, jit_propagate_error};
use crate::exec::ctx::ExecCtx;
use crate::exec::frame_ctrl::resolve_constructor_return;
use crate::value::VmValue;

/// Maximum VM call depth before a graceful error is raised. Kept in sync with
/// the interpreter guard (`exec::calls`). JIT'd calls recurse on the native
/// Rust stack (the JIT invokes the callee's `jit_entry` directly), so without
/// this guard deep recursion aborts the host process instead of producing a
/// catchable runtime error.
const MAX_CALL_DEPTH: usize = 10000;

#[inline(always)]
pub(super) unsafe fn jit_guard_call_depth(ctx: &mut ExecCtx) {
    if ctx.frames.len() >= MAX_CALL_DEPTH {
        let e = crate::error::RuntimeError::new(format!(
            "stack overflow: call depth exceeded {MAX_CALL_DEPTH}"
        ));
        jit_propagate_error(ctx, e);
    }
}

pub(crate) extern "C" fn jit_call(
    ctx: *mut ExecCtx,
    args: *const varn_jit::JitCallArgs,
) -> VmValue {
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
                let is_eligible = !closure.proto.is_async
                    && !closure.proto.is_generator
                    && !closure.proto.has_rest
                    && !closure.proto.has_try();

                if let Some(jit_fn) = closure.jit_fn().filter(|_| is_eligible) {
                    let callee_base = base + args.arg_start;

                    let required = callee_base + closure.proto.register_count as usize + 32;

                    if ctx_ref.stack.len() < required {
                        ctx_ref.stack.resize(required, VmValue::null());
                    }

                    ctx_ref
                        .frames
                        .push(crate::frame::CallFrame::new(closure, callee_base));

                    ctx_ref.jit_frame_prepushed = 1;
                    let res = (jit_fn)(
                        ctx_ref.stack.as_mut_ptr() as *mut std::ffi::c_void,
                        &**closure as *const crate::closure::VmClosure as *const std::ffi::c_void,
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
            } else if let Some(crate::heap::HeapObj::Class(cls)) = heap_obj {
                let cls = cls.clone();
                if let Some(final_val) = jit_construct_fast(ctx_ref, &cls, base, args) {
                    return final_val;
                }
            } else if let Some(crate::heap::HeapObj::NativeFn(f, name)) = heap_obj {
                let f = *f;
                let name = *name;
                ctx_ref.record_call_native(f, Some(name));
                let arg_base = base + args.arg_start;

                let result = if args.arg_count <= 1 {
                    ctx_ref.invoke_native(f, &[])
                } else {
                    let actual_count = args.arg_count - 1;
                    if actual_count <= 8 {
                        let mut buf = [VmValue::null(); 8];
                        buf[..actual_count].copy_from_slice(
                            &ctx_ref.stack[(arg_base + 1)..(arg_base + 1 + actual_count)],
                        );
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
                    while ctx_ref.frames.len() > caller_depth {
                        let f = ctx_ref.frames.pop().unwrap();
                        ctx_ref.close_upvalues_above(f.base);
                    }
                    jit_propagate_error(ctx_ref, e);
                }
            }
            Ok(false) => {}
            Err(e) => {
                while ctx_ref.frames.len() > caller_depth {
                    let f = ctx_ref.frames.pop().unwrap();
                    ctx_ref.close_upvalues_above(f.base);
                }
                jit_propagate_error(ctx_ref, e);
            }
        }

        ctx_ref.stack[base + args.dest]
    }
}

pub(crate) extern "C" fn jit_call_method(
    ctx: *mut ExecCtx,
    closure: *const crate::closure::VmClosure,
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

/// Flat-argument shim over [`jit_call_method`] for the CLIF backend.
#[allow(clippy::too_many_arguments)]
pub(crate) extern "C" fn jit_call_method_flat(
    ctx: *mut ExecCtx,
    closure: *const crate::closure::VmClosure,
    base: usize,
    this_tag: u64,
    this_payload: u64,
    name_idx: usize,
    cs: usize,
    arg_start: usize,
    arg_count: usize,
    dest: usize,
    ip: usize,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let closure_ref = &*closure;
        let caller_depth = ctx_ref.frames.len();
        let frame_idx = caller_depth - 1;
        let this_val = VmValue::from_raw_parts(this_tag, this_payload);

        if let Some(res) = super::ic::try_fast_jit_method(
            ctx_ref,
            closure_ref,
            this_val,
            base,
            name_idx,
            arg_start,
            arg_count,
        ) {
            ctx_ref.stack[base + dest] = res;
            ctx_ref.jit_native_result = res;
            return;
        }

        ctx_ref.frames[frame_idx].ip = ip;

        let res = ctx_ref.exec_call_method_reg(
            this_val,
            base,
            name_idx,
            cs,
            arg_start,
            arg_count,
            dest,
            frame_idx,
            closure_ref,
        );

        match res {
            Ok(true) => {
                if let Err(e) = ctx_ref.run_until_inner(caller_depth) {
                    while ctx_ref.frames.len() > caller_depth {
                        let f = ctx_ref.frames.pop().unwrap();
                        ctx_ref.close_upvalues_above(f.base);
                    }
                    jit_propagate_error(ctx_ref, e);
                }
            }
            Ok(false) => {}
            Err(e) => {
                while ctx_ref.frames.len() > caller_depth {
                    let f = ctx_ref.frames.pop().unwrap();
                    ctx_ref.close_upvalues_above(f.base);
                }
                jit_propagate_error(ctx_ref, e);
            }
        }

        ctx_ref.jit_native_result = ctx_ref.stack[base + dest];
    }
}

pub(crate) extern "C" fn clif_call_fallback(
    ctx: *mut ExecCtx,
    callee_tag: u64,
    callee_payload: u64,
    src: usize,
    argc: usize,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let callee = VmValue::from_raw_parts(callee_tag, callee_payload);
        if callee.is_heap() {
            let heap_obj = ctx_ref.heap.get(callee.as_heap_idx());
            if let Some(crate::heap::HeapObj::VmClosure(closure)) = heap_obj {
                let is_eligible = !closure.proto.is_async
                    && !closure.proto.is_generator
                    && !closure.proto.has_rest;
                if is_eligible {
                    let closure = closure.clone();
                    if let Some(jit_fn) = closure.hot_jit_fn() {
                        if ctx_ref.stack.len() < src + argc {
                            ctx_ref.stack.resize(src + argc, VmValue::null());
                        }
                        let orig_len = ctx_ref.stack.len();
                        ctx_ref.stack.extend_from_within(src..src + argc);
                        let callee_base = orig_len;
                        let required = callee_base + closure.proto.register_count as usize + 32;
                        if ctx_ref.stack.len() < required {
                            ctx_ref.stack.resize(required, VmValue::null());
                        }
                        ctx_ref
                            .frames
                            .push(crate::frame::CallFrame::new(&closure, callee_base));
                        ctx_ref.jit_frame_prepushed = 1;
                        let res = (jit_fn)(
                            ctx_ref.stack.as_mut_ptr() as *mut std::ffi::c_void,
                            &*closure as *const crate::closure::VmClosure
                                as *const std::ffi::c_void,
                            callee_base,
                            ctx_ref as *mut ExecCtx as *mut std::ffi::c_void,
                        );
                        let returning_frame_idx = ctx_ref.frames.len() - 1;
                        ctx_ref.frames.pop();
                        ctx_ref.close_upvalues_above(callee_base);
                        let final_val =
                            resolve_constructor_return(ctx_ref, returning_frame_idx, res);
                        ctx_ref.stack.truncate(orig_len);
                        ctx_ref.jit_native_result = final_val;
                        ctx_ref.record_call_vm_fast();
                        return;
                    }
                }
            } else if let Some(crate::heap::HeapObj::Class(cls)) = heap_obj {
                let cls = cls.clone();
                if ctx_ref.stack.len() < src + argc {
                    ctx_ref.stack.resize(src + argc, VmValue::null());
                }
                if let Some(final_val) = super::construct::construct_staged_fast(ctx_ref, &cls, src)
                {
                    ctx_ref.jit_native_result = final_val;
                    ctx_ref.record_call_vm_fast();
                    return;
                }
            }
        }
        match ctx_ref.call_vm_window(callee, src, argc) {
            Ok(v) => ctx_ref.jit_native_result = v,
            Err(err) => jit_propagate_error(ctx_ref, err),
        }
    }
}

pub(crate) extern "C" fn jit_call_spread(ctx: *mut ExecCtx, args: *const std::ffi::c_void) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let args = &*(args as *const varn_jit::JitCallArgs);
        let caller_depth = ctx_ref.frames.len();
        let frame_idx = caller_depth - 1;
        let base = ctx_ref.frames[frame_idx].base;

        ctx_ref.frames[frame_idx].ip = args.ip;

        let res = ctx_ref.exec_call_spread_reg(
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
                jit_propagate_error(ctx_ref, e);
            }
        }

        ctx_ref.jit_native_result = ctx_ref.stack[base + args.dest];
    }
}
