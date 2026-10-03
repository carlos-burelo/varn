//! Calling VM code from compiled code.

use super::construct::jit_propagate_error;
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;

/// Fast lane for a JIT-guarded method call on a class instance.
///
/// The compiled caller already proved `receiver.class_id == entry_id` with an
/// inline heap walk and loaded this entry's `(class_ptr, slot, kind, ver)`
/// from its own cache slot — no name lookup, no registry, no string work.
/// This re-verifies the cheap parts (class pointer liveness is structural:
/// the entry's `Rc` keeps it alive; the id/version/slot checks below close
/// the eviction race) and dispatches the vtable method directly. Anything
/// unexpected — a stale entry, a generator/async method, an arity mismatch,
/// a vtable shape this lane does not serve — degrades to the canonical
/// [`ExecCtx::call_method`], so the observable semantics are exactly the
/// slow path's by construction.
///
/// `window[0]` is the receiver, `window[1..]` the arguments, boxed; the
/// result lands in `jit_native_result` like every other windowed helper.
#[allow(clippy::too_many_arguments)]
#[varn_op_macros::jit_slow(field = "jit_call_method_cached_window")]
pub(crate) extern "C" fn jit_call_method_cached_window(
    ctx: *mut ExecCtx,
    class_ptr: usize,
    entry_id: u32,
    slot: u16,
    kind: u8,
    ver: u8,
    name_idx: usize,
    cs: usize,
    window: *const VmValue,
    total: usize,
) {
    use std::sync::atomic::Ordering;
    use varn_types::chunk::ICKind;
    unsafe {
        let ctx_ref = &mut *ctx;
        let caller_depth = ctx_ref.frames.len();
        let frame_idx = caller_depth - 1;
        let closure_ref = &*ctx_ref.frames[frame_idx].closure_ptr;
        let window = std::slice::from_raw_parts(window, total);
        let args = crate::exec::method_args::MethodArgs::Boxed(&window[1..]);
        let this_val = window[0];

        // Fast lane: instance receiver, live class entry, current version.
        // Enum variants and intrinsics never apply to instances (the
        // interpreter's own `intrinsic_method` answers `None` for them), so
        // skipping those checks here changes nothing observable.
        //
        // `class_ptr` is the entry's `Option<Rc<ClassObj>>` payload — the
        // `ClassObj` value pointer, non-null exactly when the entry carries
        // a class. The entry's own `Rc` keeps the allocation alive for the
        // whole helper (single-threaded runtime, entry owned by the running
        // closure's cache), so borrowing through it is sound; an owned `Rc`
        // for the callee frame is rebuilt with a balanced retain below.
        let fast = (|| {
            if class_ptr == 0
                || (kind != ICKind::NATIVE_VTABLE_METHOD && kind != ICKind::VM_VTABLE_METHOD)
                || !this_val.is_heap()
            {
                return None;
            }
            let crate::heap::HeapObj::Instance(inst) = ctx_ref.heap.get(this_val.as_heap_idx())?
            else {
                return None;
            };
            if inst.class_id != entry_id {
                return None;
            }
            let cls = &*(class_ptr as *const varn_types::value::ClassObj);
            if cls.id != entry_id
                || (cls.vtable_version.load(Ordering::Relaxed) & 0xFF) as u8 != ver
            {
                return None;
            }
            let vtable = &*cls.vtable.as_ptr();
            let method = *vtable.get(slot as usize)?;
            if kind == ICKind::NATIVE_VTABLE_METHOD {
                let (f, _) = ctx_ref.heap.native_of(method)?;
                Some(FastMethod::Native(f))
            } else {
                let nc = ctx_ref.heap.closure_of(method)?;
                if nc.proto.is_generator || nc.proto.is_async || args.len() > nc.proto.arity {
                    return None;
                }
                // Balanced retain: `from_raw` borrows the entry's share and
                // `forget` suppresses its release, so the clone nets exactly
                // +1 with no leak and no registry round-trip.
                let owned: std::rc::Rc<varn_types::value::ClassObj> = {
                    let rc = std::rc::Rc::from_raw(class_ptr as *const varn_types::value::ClassObj);
                    let out = std::rc::Rc::clone(&rc);
                    std::mem::forget(rc);
                    out
                };
                Some(FastMethod::Vm(nc.clone(), owned))
            }
        })();

        enum FastMethod {
            Native(varn_types::NativeFn),
            Vm(
                std::rc::Rc<crate::closure::VmClosure>,
                std::rc::Rc<varn_types::value::ClassObj>,
            ),
        }
        let outcome = match fast {
            Some(FastMethod::Native(f)) => {
                ctx_ref.record_ic_hit_callmethod();
                ctx_ref.call_native_method(f, this_val, args, "")
            }
            Some(FastMethod::Vm(nc, owner_rc)) => {
                ctx_ref.record_ic_hit_callmethod();
                ctx_ref.invoke_vm_method_fast(nc, Some(owner_rc), this_val, args, "")
            }
            None => {
                return fallback_method_call(
                    ctx_ref,
                    caller_depth,
                    frame_idx,
                    closure_ref,
                    this_val,
                    name_idx,
                    cs,
                    args,
                )
            }
        };
        finish_method_outcome(ctx_ref, caller_depth, outcome);
    }
}

/// The canonical tail every windowed method helper shares: a value lands in
/// `jit_native_result`, a pushed VM frame runs to completion, an error unwinds
/// exactly like the slow path's.
fn finish_method_outcome(
    ctx_ref: &mut ExecCtx,
    caller_depth: usize,
    outcome: Result<crate::exec::method_args::MethodOutcome, crate::error::RuntimeError>,
) {
    let result = match outcome {
        Ok(crate::exec::method_args::MethodOutcome::Value(v)) => Ok(v),
        Ok(crate::exec::method_args::MethodOutcome::FramePushed) => ctx_ref.run_until(caller_depth),
        Err(e) => Err(e),
    };
    match result {
        Ok(v) => ctx_ref.jit_native_result = v,
        Err(e) => unsafe {
            while ctx_ref.frames.len() > caller_depth {
                let f = ctx_ref.frames.pop().unwrap();
                ctx_ref.close_upvalues_in(f.base);
            }
            jit_propagate_error(ctx_ref, e);
        },
    }
}

/// Full-resolution fallback for the cached lane: the interpreter's own
/// `call_method`, with the same epilogue as [`jit_call_method_window`].
#[allow(clippy::too_many_arguments)]
fn fallback_method_call(
    ctx_ref: &mut ExecCtx,
    caller_depth: usize,
    frame_idx: usize,
    closure_ref: &crate::closure::VmClosure,
    this_val: VmValue,
    name_idx: usize,
    cs: usize,
    args: crate::exec::method_args::MethodArgs<'_>,
) {
    let outcome = ctx_ref.call_method(this_val, name_idx, cs, args, frame_idx, closure_ref);
    finish_method_outcome(ctx_ref, caller_depth, outcome);
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
