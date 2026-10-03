use super::construct::jit_propagate_error;
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;

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
