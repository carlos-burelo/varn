//! Calling VM code from compiled code.

use super::construct::jit_propagate_error;
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;

/// Maximum VM call depth before a graceful error is raised. Kept in sync with
/// the interpreter guard (`exec::calls`). JIT'd calls recurse on the native
/// Rust stack (the JIT invokes the callee's `jit_entry` directly), so without
/// this guard deep recursion aborts the host process instead of producing a
/// catchable runtime error.
pub(crate) const MAX_CALL_DEPTH: usize = 10000;

/// Flat-argument method call for the CLIF backend. The
/// compiled caller flushed its args to the caller activation's homes
/// (`base` = act_id, `arg_start` = first argument register), which is exactly
/// what `exec_call_method_reg` reads, so this forwards to it and runs any
/// frame it pushed to completion.
#[allow(clippy::too_many_arguments)]
#[varn_op_macros::jit_slow(field = "call_method_flat")]
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

/// A method call out of the lowering from typed SSA: `window` holds the
/// receiver then the arguments, boxed, and `name_idx` / `cs` are the calling
/// function's constant and cache slot. It runs the interpreter's own
/// [`ExecCtx::call_method`] (one resolution, one inline cache) on those
/// values and runs a VM method it pushes to completion. Every heap value in
/// the window is also in its SSA value's home, a GC root, for the call.
#[varn_op_macros::jit_slow(field = "jit_call_method_window")]
pub(crate) extern "C" fn jit_call_method_window(
    ctx: *mut ExecCtx,
    name_idx: usize,
    cs: usize,
    window: *const VmValue,
    total: usize,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let caller_depth = ctx_ref.frames.len();
        let frame_idx = caller_depth - 1;
        let closure_ref = &*ctx_ref.frames[frame_idx].closure_ptr;
        let window = std::slice::from_raw_parts(window, total);
        let args = crate::exec::method_args::MethodArgs::Boxed(&window[1..]);
        let outcome = ctx_ref.call_method(window[0], name_idx, cs, args, frame_idx, closure_ref);
        let result = match outcome {
            Ok(crate::exec::method_args::MethodOutcome::Value(v)) => Ok(v),
            Ok(crate::exec::method_args::MethodOutcome::FramePushed) => {
                ctx_ref.run_until(caller_depth)
            }
            Err(e) => Err(e),
        };
        match result {
            Ok(v) => ctx_ref.jit_native_result = v,
            Err(e) => {
                while ctx_ref.frames.len() > caller_depth {
                    let f = ctx_ref.frames.pop().unwrap();
                    ctx_ref.close_upvalues_in(f.base);
                }
                jit_propagate_error(ctx_ref, e);
            }
        }
    }
}

/// Camino dinámico ÚNICO v2 (§3.2): todo lo no-estático (métodos, closures,
/// `dynamic`) pasa por aquí. Ventana contigua ya preparada por el caller en
/// sus homes; resuelve, ejecuta (entrando a código compilado si existe) y deja
/// boxed en `ctx.jit_native_result` (el retorno directo `-> VmValue` llega con
/// la convención por target, §3.1). Un call-site monomórfico caliente se
/// recompila a estático directo; sin IC inlineado a mano por call-site.
#[varn_op_macros::jit_slow(field = "invoke_dynamic")]
pub(crate) extern "C" fn jit_invoke_dynamic(
    ctx: *mut ExecCtx,
    callee_tag: u64,
    callee_payload: u64,
    act_id: usize,
    arg_start: usize,
    argc: u32,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let callee = VmValue::from_raw_parts(callee_tag, callee_payload);
        let window = ctx_ref.stack.box_range(act_id, arg_start, argc as usize);
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
/// `jit_invoke_dynamic`, so there is still one invocation, and writes the
/// boxed result to `ctx.jit_native_result`.
#[varn_op_macros::jit_slow(field = "jit_invoke_window")]
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
/// `jit_invoke_dynamic` still does the ENTIRE thing in one FFI
/// hop for anything this declines: async/generator/rest closures, class
/// construction, native functions, or a closure with no compiled code (yet,
/// or ever).
///
/// On success, pushes a `CallFrame`, writes the frame's `base` and the
/// resolved closure's address to the out-params (the call site cannot compute
/// either ahead of the call: `base` is wherever the stack lands, the closure
/// address needs a heap lookup) and returns the callee's wrapper entry point
/// (a `JitFn`, as a raw address) for the call site to invoke directly.
/// Returns `0` on decline; nothing is pushed or written, and the call site
/// must fall back to `jit_invoke_dynamic`. A non-zero return always has a
/// matching `jit_finish_static_call` after the wrapper call — the frame stays
/// pushed until then.
/// Fase B: pushes a fresh `FrameStore` activation for the callee, copies the
/// argument registers from the caller activation's homes with `mov_cross`,
/// pushes its `CallFrame`, and returns the callee's compiled wrapper entry so
/// the caller invokes it directly (no Rust [`ExecCtx::invoke`] round-trip).
/// Returns `0` — nothing pushed — for a non-closure, async/generator/rest, or
/// a callee with no compiled entry; the call site then falls back to
/// `jit_invoke_dynamic`.
/// `dest` (registro destino del caller) viaja explícito y se estampa como
/// `return_reg` del callee — nunca por campo compartido (§3.3, cero stores
/// por llamada).
///
/// Salidas por out-params explícitos (slot de 16 B del caller nativo), nunca
/// por campos scratch de `ExecCtx`: `out_closure`/`out_base` reciben el
/// closure resuelto y la base del callee; el retorno es la entry del wrapper
/// (`0` = declina). Un wrapper anidado ya no puede pisar lo que el exterior
/// aún no leyó — la hazard documentada en `jit_finish_static_call` muere aquí.
#[varn_op_macros::jit_slow]
#[allow(clippy::too_many_arguments)]
#[varn_op_macros::jit_slow(field = "jit_prepare_static_call")]
pub(crate) extern "C" fn jit_prepare_static_call(
    ctx: *mut ExecCtx,
    closure_tag: u64,
    closure_payload: u64,
    act_id: usize,
    arg_start: usize,
    arg_count: usize,
    dest: usize,
    out_closure: *mut usize,
    out_base: *mut usize,
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
        frame.return_reg = dest as u16;
        let closure_ptr = frame.closure_ptr as usize;
        ctx_ref.frames.push(frame);
        // Out-params al slot nativo del call-site (siempre válidos); solo se
        // escriben en éxito — con `0` el call-site toma el lento sin leerlos.
        out_closure.write(closure_ptr);
        out_base.write(callee_alloc);
        jit_fn as usize
    }
}

/// Pops the activation [`jit_prepare_static_call`] pushed and closes its
/// upvalues. `callee_alloc` travels as an explicit argument (the CLIF call
/// site's own out-param slot value).
#[varn_op_macros::jit_slow(field = "jit_finish_static_call")]
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
#[varn_op_macros::jit_slow(field = "clif_call_self")]
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
#[varn_op_macros::jit_slow(field = "jit_call_self_window")]
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
