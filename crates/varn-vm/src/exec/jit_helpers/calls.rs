//! Calling VM code from compiled code.
//!
//! K3-faseA: TODO este archivo entero. Solo lo invoca código generado, que ya
//! no existe (`FRAME_LAYOUT_V2_JIT_BAIL` impide compilar): cada helper que
//! tocaba el layout `Vec<VmValue>` contiguo queda invalidado con un tripwire
//! explícito. La fase B lo restaura de git junto con el lowering al layout
//! por clases. Se conservan firmas, ABI y `MAX_CALL_DEPTH` para que la tabla
//! de helpers (`jit::helpers`, `helper_abi`) siga enlazando.

use super::construct::jit_propagate_error;
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;

/// Maximum VM call depth before a graceful error is raised. Kept in sync with
/// the interpreter guard (`exec::calls`). JIT'd calls recurse on the native
/// Rust stack (the JIT invokes the callee's `jit_entry` directly), so without
/// this guard deep recursion aborts the host process instead of producing a
/// catchable runtime error.
pub(crate) const MAX_CALL_DEPTH: usize = 10000;

macro_rules! bailed {
    () => {
        unreachable!("K3-faseA: helper de código compilado; ver FRAME_LAYOUT_V2_JIT_BAIL")
    };
}

pub(crate) extern "C" fn jit_call(
    ctx: *mut ExecCtx,
    args: *const varn_jit::JitCallArgs,
) -> VmValue {
    let _ = (ctx, args);
    bailed!()
}

pub(crate) extern "C" fn jit_call_method(
    ctx: *mut ExecCtx,
    closure: *const crate::closure::VmClosure,
    args: *const varn_jit::JitCallMethodArgs,
) -> VmValue {
    let _ = (ctx, closure, args);
    bailed!()
}

/// Flat-argument shim over [`jit_call_method`] for the CLIF backend. The
/// compiled caller flushed its args to the caller activation's homes
/// (`base` = act_id, `arg_start` = first argument register), which is exactly
/// what `exec_call_method_reg` reads, so this forwards to it and runs any
/// frame it pushed to completion.
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
                        ctx_ref.close_upvalues_in(f.base);
                    }
                    jit_propagate_error(ctx_ref, e);
                }
            }
            Ok(false) => {}
            Err(e) => {
                while ctx_ref.frames.len() > caller_depth {
                    let f = ctx_ref.frames.pop().unwrap();
                    ctx_ref.close_upvalues_in(f.base);
                }
                jit_propagate_error(ctx_ref, e);
            }
        }

        ctx_ref.jit_native_result = ctx_ref.stack.box_reg(base, dest);
    }
}

/// The single canonical VM call out of compiled code. The compiled caller has
/// flushed `[callee, args...]` to the homes of registers
/// `arg_start..arg_start + argc` in activation `act_id`; this gathers them and
/// runs the callee through [`ExecCtx::invoke`], the same entry the interpreter
/// slow path and every host→VM call use.
pub(crate) extern "C" fn clif_call_fallback(
    ctx: *mut ExecCtx,
    callee_tag: u64,
    callee_payload: u64,
    act_id: usize,
    arg_start: usize,
    argc: usize,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let callee = VmValue::from_raw_parts(callee_tag, callee_payload);
        let window = ctx_ref.stack.box_range(act_id, arg_start, argc);
        match ctx_ref.invoke(callee, &window) {
            Ok(v) => ctx_ref.jit_native_result = v,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

/// `extern "C" fn(ctx, callee_tag, callee_payload, window: *const VmValue, argc)`
/// — the SSA lowering's fallback for a call whose caller has no VM activation
/// of its own to keep its argument window in. `window[0]` is the callee
/// placeholder, `window[1..]` the arguments (built on the caller's native
/// stack); this runs the callee through the SAME [`ExecCtx::invoke`] as
/// `clif_call_fallback`, so there is still one invocation, and writes the
/// boxed result to `ctx.jit_native_result`.
pub(crate) extern "C" fn jit_invoke_window(
    ctx: *mut ExecCtx,
    callee_tag: u64,
    callee_payload: u64,
    window: *const VmValue,
    argc: usize,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let callee = VmValue::from_raw_parts(callee_tag, callee_payload);
        let window = std::slice::from_raw_parts(window, argc);
        match ctx_ref.invoke(callee, window) {
            Ok(v) => ctx_ref.jit_native_result = v,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

/// Half of the compiled-call fast path, split so a frame-aware `Call`'s CLIF
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
/// Fase B: pushes a fresh `FrameStore` activation for the callee, copies the
/// argument registers from the caller activation's homes with `mov_cross`,
/// pushes its `CallFrame`, and returns the callee's compiled wrapper entry so
/// the caller invokes it directly (no Rust [`ExecCtx::invoke`] round-trip).
/// Returns `0` — nothing pushed — for a non-closure, async/generator/rest, or
/// a callee with no compiled entry; the call site then falls back to
/// `clif_call_fallback`.
pub(crate) extern "C" fn jit_prepare_static_call(
    ctx: *mut ExecCtx,
    closure_tag: u64,
    closure_payload: u64,
    act_id: usize,
    arg_start: usize,
    arg_count: usize,
) -> usize {
    unsafe {
        let ctx_ref = &mut *ctx;
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
        let Some(jit_fn) = closure.hot_jit_fn() else {
            return 0;
        };
        let closure = closure.clone();

        let callee_alloc =
            match ctx_ref.push_call_frame(&closure.proto, act_id, arg_start, arg_count) {
                Ok(a) => a,
                Err(e) => jit_propagate_error(ctx_ref, e),
            };
        if crate::home_trace::enabled() {
            let copied: Vec<String> = (0..arg_count)
                .map(|i| {
                    let v = ctx_ref.stack.box_reg(callee_alloc, i);
                    format!("{:#x}/{:#x}", v.raw_tag(), v.raw_payload())
                })
                .collect();
            eprintln!(
                "PREPCALL callee={:?} act={act_id} arg_start={arg_start} argc={arg_count} copied={copied:?}",
                closure.proto.name
            );
        }
        let mut frame = crate::frame::CallFrame::new_owned(closure, callee_alloc);
        frame.return_reg = ctx_ref.jit_call_dest as u16;
        let closure_ptr = frame.closure_ptr as usize;
        ctx_ref.frames.push(frame);
        ctx_ref.jit_frame_prepushed = 1;
        ctx_ref.jit_call_base = callee_alloc;
        ctx_ref.jit_call_closure_ptr = closure_ptr;
        jit_fn as usize
    }
}

/// Pops the activation [`jit_prepare_static_call`] pushed and closes its
/// upvalues. `callee_alloc` travels as an explicit argument (the CLIF call
/// site's own captured value), never re-read from `jit_call_base` — a nested
/// call in the wrapper can overwrite that shared field.
pub(crate) extern "C" fn jit_finish_static_call(ctx: *mut ExecCtx, callee_alloc: usize) {
    unsafe {
        let ctx_ref = &mut *ctx;
        ctx_ref.frames.pop();
        ctx_ref.close_upvalues_in(callee_alloc);
        ctx_ref.stack.pop_frame();
    }
}

/// Direct self-recursion out of a frame-aware lowering: the caller pushes a
/// fresh activation for its OWN closure and copies the `argc` argument
/// registers from its homes into the callee's, then runs it to completion.
/// `act_id` is the caller's activation.
pub(crate) extern "C" fn clif_call_self(
    ctx: *mut ExecCtx,
    act_id: usize,
    arg_start: usize,
    argc: usize,
) {
    unsafe {
        call_running_closure(&mut *ctx, argc, |ctx, callee, i| {
            ctx.stack.mov_cross(callee, i, act_id, arg_start + i)
        });
    }
}

/// As [`clif_call_self`], for a caller whose arguments are not in contiguous
/// homes (the lowering from typed SSA): they arrive as a boxed `window`,
/// placeholder first. The window lives on the native stack, which the
/// collector does not see, and pushing a frame can collect, so it is copied
/// into staging — a root — before anything else.
pub(crate) extern "C" fn jit_call_self_window(
    ctx: *mut ExecCtx,
    window: *const VmValue,
    argc: usize,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        ctx_ref.stage.clear();
        ctx_ref
            .stage
            .extend_from_slice(std::slice::from_raw_parts(window, argc));
        call_running_closure(ctx_ref, argc, |ctx, callee, i| {
            let v = ctx.stage[i];
            let addr = ctx.stack.addr_of(callee, i);
            ctx.stack.set_addr(addr, v)
        });
    }
}

/// Push a fresh activation of the running closure, fill its `argc` argument
/// registers with `place(ctx, callee_activation, i)`, and run it to
/// completion; the result goes to `jit_native_result`.
unsafe fn call_running_closure(
    ctx_ref: &mut ExecCtx,
    argc: usize,
    mut place: impl FnMut(&mut ExecCtx, usize, usize) -> crate::error::VmResult<()>,
) {
    let caller_depth = ctx_ref.frames.len();
    let frame_idx = caller_depth - 1;
    // The running closure is borrowed, not owned: `CallFrame::new` keeps it
    // alive through the caller's frame (frames form a chain). Avoids
    // requiring `_owned_closure`, which the entry frame may not carry.
    let closure_ptr = ctx_ref.frames[frame_idx].closure_ptr;
    let closure_ref = &*closure_ptr;
    let proto = closure_ref.proto.clone();
    let callee_alloc = ctx_ref.stack.push_frame(&proto);
    for i in 0..argc {
        if let Err(e) = place(ctx_ref, callee_alloc, i) {
            ctx_ref.stack.pop_frame();
            ctx_ref.stage.clear();
            jit_propagate_error(ctx_ref, e);
        }
    }
    // The arguments are in the callee's homes now; staging is free for the
    // calls the callee makes.
    ctx_ref.stage.clear();
    ctx_ref
        .frames
        .push(crate::frame::CallFrame::new(closure_ref, callee_alloc));
    match ctx_ref.run_until(caller_depth) {
        Ok(v) => ctx_ref.jit_native_result = v,
        Err(e) => jit_propagate_error(ctx_ref, e),
    }
}

pub(crate) extern "C" fn jit_call_spread(ctx: *mut ExecCtx, args: *const std::ffi::c_void) {
    let _ = (ctx, args);
    bailed!()
}
