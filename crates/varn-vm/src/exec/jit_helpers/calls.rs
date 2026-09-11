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
                let closure = closure.clone();
                let is_eligible = !closure.proto.is_async
                    && !closure.proto.is_generator
                    && !closure.proto.has_rest
                    && !closure.proto.has_try();

                if let Some(jit_fn) = closure.jit_fn().filter(|_| is_eligible) {
                    let callee_base = base + args.arg_start;

                    let required_len = callee_base + closure.proto.register_count as usize;
                    let required_cap = required_len + 32;
                    if ctx_ref.stack.capacity() < required_cap {
                        ctx_ref
                            .stack
                            .reserve((required_cap - ctx_ref.stack.len()).max(256));
                    }
                    if ctx_ref.stack.len() < required_len {
                        ctx_ref.stack.resize(required_len, VmValue::null());
                    }

                    ctx_ref
                        .frames
                        .push(crate::frame::CallFrame::new(&closure, callee_base));

                    ctx_ref.jit_frame_prepushed = 1;
                    let res = (jit_fn)(
                        ctx_ref.stack.as_mut_ptr() as *mut std::ffi::c_void,
                        &*closure as *const crate::closure::VmClosure as *const std::ffi::c_void,
                        callee_base,
                        ctx_ref as *mut ExecCtx as *mut std::ffi::c_void,
                    );

                    let returning_frame_idx = ctx_ref.frames.len() - 1;

                    ctx_ref.frames.pop();

                    if closure.proto.upvalue_count > 0 {
                        ctx_ref.close_upvalues_above(callee_base);
                    }

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
                    if invoke_compiled_closure(ctx_ref, &closure, src, argc) {
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

/// Calls `closure`'s compiled entry with `argc` values copied from `stack[src]`,
/// on a frame of its own pushed above the caller's, and leaves the result in
/// `ctx.jit_native_result`. Returns false — having done nothing — when the
/// closure has no compiled entry yet.
///
/// The one place that knows the JIT frame protocol: every compiled caller
/// reaches a compiled callee through it, whether the callee was resolved from a
/// value or is the caller itself recursing.
unsafe fn invoke_compiled_closure(
    ctx: &mut ExecCtx,
    closure: &crate::closure::VmClosure,
    src: usize,
    argc: usize,
) -> bool {
    let Some(jit_fn) = closure.hot_jit_fn() else {
        return false;
    };
    if ctx.stack.len() < src + argc {
        ctx.stack.resize(src + argc, VmValue::null());
    }
    let orig_len = ctx.stack.len();
    ctx.stack.extend_from_within(src..src + argc);
    let callee_base = orig_len;
    let required_len = callee_base + closure.proto.register_count as usize;
    let required_cap = required_len + 32;
    if ctx.stack.capacity() < required_cap {
        ctx.stack.reserve((required_cap - ctx.stack.len()).max(256));
    }
    if ctx.stack.len() < required_len {
        ctx.stack.resize(required_len, VmValue::null());
    }
    ctx.frames
        .push(crate::frame::CallFrame::new(closure, callee_base));
    ctx.jit_frame_prepushed = 1;
    let res = (jit_fn)(
        ctx.stack.as_mut_ptr() as *mut std::ffi::c_void,
        closure as *const crate::closure::VmClosure as *const std::ffi::c_void,
        callee_base,
        ctx as *mut ExecCtx as *mut std::ffi::c_void,
    );
    let returning_frame_idx = ctx.frames.len() - 1;
    ctx.frames.pop();
    ctx.close_upvalues_above(callee_base);
    let final_val = resolve_constructor_return(ctx, returning_frame_idx, res);
    ctx.stack.truncate(orig_len);
    ctx.jit_native_result = final_val;
    ctx.record_call_vm_fast();
    true
}

/// Half of `invoke_compiled_closure`, split so a frame-aware `Call`'s CLIF
/// call site can make the one machine call that matters — into the callee's
/// own compiled wrapper — itself, instead of crossing back into Rust only to
/// have Rust make that same call through a bare function pointer.
/// `clif_call_fallback` (unchanged) still does the ENTIRE thing in one FFI
/// hop for anything this declines: async/generator/rest closures, class
/// construction, native functions, or a closure with no compiled code (yet,
/// or ever).
///
/// On success, pushes a `CallFrame` and writes the frame's `base` and the
/// resolved closure's address to `ctx.jit_call_base` /
/// `ctx.jit_call_closure_ptr` (the call site cannot compute either ahead of
/// the call: `base` is wherever `ctx.stack.len()` lands, the closure address
/// needs a heap lookup) and returns the callee's wrapper entry point (a
/// `JitFn`, as a raw address) for the call site to invoke directly. Returns
/// `0` on decline; nothing is pushed, and the call site must fall back to
/// `clif_call_fallback`. A non-zero return always has a matching
/// `jit_finish_static_call` after the wrapper call — the frame stays pushed
/// until then.
pub(crate) extern "C" fn jit_prepare_static_call(
    ctx: *mut ExecCtx,
    closure_tag: u64,
    closure_payload: u64,
    arg_start: usize,
    arg_count: usize,
) -> usize {
    unsafe {
        let ctx_ref = &mut *ctx;
        // Record the caller's post-call resume ip/dest, written to
        // `jit_resume_ip`/`jit_call_dest` by the CLIF call site right before
        // this call: if the callee (or a deeper frame) throws and the
        // exception is caught below this caller, the longjmp unwinds this
        // caller's native JIT frame; the interpreter then resumes it
        // *interpreted* from this ip instead of re-running its JIT body from
        // ip=0 (which loops forever). Mirrors `jit_prepare_call`.
        let resume_ip = ctx_ref.jit_resume_ip;
        let call_dest = ctx_ref.jit_call_dest as u16;
        if let Some(caller) = ctx_ref.frames.last_mut() {
            caller.ip = resume_ip;
        }
        let callee = VmValue::from_raw_parts(closure_tag, closure_payload);
        if !callee.is_heap() {
            return 0;
        }
        let Some(crate::heap::HeapObj::VmClosure(closure)) = ctx_ref.heap.get(callee.as_heap_idx())
        else {
            return 0;
        };
        if closure.proto.is_async || closure.proto.is_generator || closure.proto.has_rest {
            return 0;
        }
        // `hot_jit_fn`, not `jit_fn`: a callee below its tiering threshold
        // still gets counted here, same as it would going through
        // `clif_call_fallback` — this path only changes how an ALREADY
        // compiled callee is invoked, not when one becomes compiled.
        let Some(jit_fn) = closure.hot_jit_fn() else {
            return 0;
        };
        let closure = closure.clone();
        jit_guard_call_depth(ctx_ref);
        if ctx_ref.stack.len() < arg_start + arg_count {
            ctx_ref.stack.resize(arg_start + arg_count, VmValue::null());
        }
        let callee_base = ctx_ref.stack.len();
        ctx_ref.stack.extend_from_within(arg_start..arg_start + arg_count);
        let required_len = callee_base + closure.proto.register_count as usize;
        let required_cap = required_len + 32;
        if ctx_ref.stack.capacity() < required_cap {
            ctx_ref.stack.reserve((required_cap - ctx_ref.stack.len()).max(256));
        }
        if ctx_ref.stack.len() < required_len {
            ctx_ref.stack.resize(required_len, VmValue::null());
        }
        let mut frame = crate::frame::CallFrame::new(&closure, callee_base);
        frame.return_reg = Some(call_dest);
        let closure_ptr = frame.closure_ptr as usize;
        // The frame must keep the closure alive on its own: the call site
        // makes the actual wrapper call itself, after this function (and its
        // local `closure` binding) has already returned.
        frame._owned_closure = Some(closure);
        ctx_ref.frames.push(frame);
        ctx_ref.jit_frame_prepushed = 1;
        ctx_ref.record_call_vm_fast();
        ctx_ref.jit_call_base = callee_base;
        ctx_ref.jit_call_closure_ptr = closure_ptr;
        jit_fn as usize
    }
}

/// The other half: pops the frame `jit_prepare_static_call` pushed, closes
/// any upvalues captured out of it, and drops the scratch call window the
/// wrapper's arguments were staged in. The wrapper call's RESULT is not this
/// function's concern — the CLIF call site reads it straight out of the
/// `call_indirect`'s own return values (or the sret slot on Windows), same
/// as it would any other typed call.
///
/// `callee_base` comes in as an explicit argument — the CLIF call site's own
/// SSA value, captured right after `jit_prepare_static_call` returned it via
/// `ctx.jit_call_base` — rather than being re-read from that shared field
/// here. The wrapper call in between can run arbitrarily deep code that
/// itself calls through this same pair, overwriting `ctx.jit_call_base` for
/// its own use; re-reading it here after that nested traffic would pop and
/// truncate to the WRONG (innermost, already-consumed) base instead of this
/// call's own.
pub(crate) extern "C" fn jit_finish_static_call(ctx: *mut ExecCtx, callee_base: usize) {
    unsafe {
        let ctx_ref = &mut *ctx;
        ctx_ref.frames.pop();
        ctx_ref.close_upvalues_above(callee_base);
        ctx_ref.stack.truncate(callee_base);
    }
}

/// Direct self-recursion out of a frame-aware lowering. The caller cannot hand
/// the callee its own `base` — it would write its home slots over the caller's
/// live ones — and it has no boxed callee to route through `clif_call_fallback`,
/// so it names the closure it is already running.
pub(crate) extern "C" fn clif_call_self(ctx: *mut ExecCtx, src: usize, argc: usize) {
    unsafe {
        let ctx_ref = &mut *ctx;
        jit_guard_call_depth(ctx_ref);
        // The frame's closure outlives the call, so the pointer is taken out
        // before `ctx` is borrowed mutably.
        let Some(closure_ptr) = ctx_ref.frames.last().map(|f| f.closure() as *const _) else {
            return;
        };
        if !invoke_compiled_closure(ctx_ref, &*closure_ptr, src, argc) {
            // Unreachable by construction: the caller is executing this very
            // closure's compiled entry, so it is published.
            let e = crate::error::RuntimeError::new(
                "CallSelf: the running closure has no compiled entry",
            );
            jit_propagate_error(ctx_ref, e);
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
