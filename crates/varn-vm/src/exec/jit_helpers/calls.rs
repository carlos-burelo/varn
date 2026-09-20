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

#[inline(always)]
pub(super) unsafe fn jit_guard_call_depth(ctx: &mut ExecCtx) {
    if ctx.frames.len() >= MAX_CALL_DEPTH {
        let e = crate::error::RuntimeError::new(format!(
            "stack overflow: call depth exceeded {MAX_CALL_DEPTH}"
        ));
        jit_propagate_error(ctx, e);
    }
}

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
    let _ = (
        ctx,
        closure,
        base,
        this_tag,
        this_payload,
        name_idx,
        cs,
        arg_start,
        arg_count,
        dest,
        ip,
    );
    bailed!()
}

pub(crate) extern "C" fn clif_call_fallback(
    ctx: *mut ExecCtx,
    callee_tag: u64,
    callee_payload: u64,
    src: usize,
    argc: usize,
) {
    let _ = (ctx, callee_tag, callee_payload, src, argc);
    bailed!()
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
    let _ = (ctx, closure, src, argc);
    bailed!()
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
    let _ = (ctx, closure_tag, closure_payload, arg_start, arg_count);
    bailed!()
}

pub(crate) extern "C" fn jit_finish_static_call(ctx: *mut ExecCtx, callee_base: usize) {
    let _ = (ctx, callee_base);
    bailed!()
}

pub(crate) extern "C" fn clif_call_self(ctx: *mut ExecCtx, src: usize, argc: usize) {
    let _ = (ctx, src, argc);
    bailed!()
}

pub(crate) extern "C" fn jit_call_spread(ctx: *mut ExecCtx, args: *const std::ffi::c_void) {
    let _ = (ctx, args);
    bailed!()
}
