//! Inline-cache-backed dispatch: virtual invocation and the monomorphic
//! property fast paths.
//!
//! These are the helpers that read a cache slot first and only fall back to a
//! full lookup on a miss.

use super::construct::jit_propagate_error;
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;
use varn_types::chunk::ICKind;

pub(crate) extern "C" fn jit_invoke_virtual(
    ctx: *mut ExecCtx,
    closure: *const crate::closure::VmClosure,
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

        let res = ctx_ref.exec_call_method_reg(
            args.this_val,
            base,
            args.name_idx,
            usize::MAX,
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

/// Flat-argument shim over [`jit_invoke_virtual`] for the CLIF backend.
#[allow(clippy::too_many_arguments)]
pub(crate) extern "C" fn jit_invoke_virtual_flat(
    ctx: *mut ExecCtx,
    closure: *const crate::closure::VmClosure,
    this_tag: u64,
    this_payload: u64,
    name_idx: usize,
    arg_start: usize,
    arg_count: usize,
    dest: usize,
    ip: usize,
) {
    let this_val = VmValue::from_raw_parts(this_tag, this_payload);
    let args = varn_jit::JitInvokeVirtualArgs {
        this_val,
        name_idx,
        arg_start,
        arg_count,
        dest,
        ip,
    };
    let val = jit_invoke_virtual(ctx, closure, &args);
    unsafe {
        (*ctx).jit_native_result = val;
    }
}

pub(crate) extern "C" fn jit_get_property_ic_fast(
    ctx: *mut ExecCtx,
    closure: *const crate::closure::VmClosure,
    obj: VmValue,
    cs_idx: usize,
) -> VmValue {
    unsafe {
        let ctx_ref = &*ctx;
        let closure_ref = &*closure;
        if cs_idx < closure_ref.ic_cache_len() {
            if obj.is_heap() {
                if let Some(crate::heap::HeapObj::Object(o)) = ctx_ref.heap.get(obj.as_heap_idx()) {
                    let guard = o.read();
                    let slot_cache = &*closure_ref.ic_cache.as_ptr();
                    let poly_slot = &slot_cache[cs_idx];
                    for entry in &poly_slot.entries {
                        if entry.id != 0
                            && entry.is_class == ICKind::SHAPE_PROP
                            && guard.shape().id == entry.id
                        {
                            if let Some(v) = guard.field_at(entry.slot as usize) {
                                return v;
                            }
                        }
                    }
                }
            }

            // A `.length` IC entry (ARRAY_LENGTH or STR_LENGTH) at this site means the
            // interpreter already validated the semantics; the receiver's own
            // tag is the guard, so no class resolution is needed.
            let slot_cache = &*closure_ref.ic_cache.as_ptr();
            let poly_slot = &slot_cache[cs_idx];
            for entry in &poly_slot.entries {
                if entry.id != 0
                    && (entry.is_class == ICKind::ARRAY_LENGTH
                        || entry.is_class == ICKind::STR_LENGTH)
                {
                    if let Some(v) = crate::exec::strings::fast_length(obj, &ctx_ref.heap) {
                        return v;
                    }
                    break;
                }
            }
        }
        VmValue::ic_miss()
    }
}

pub(crate) extern "C" fn jit_get_property_maybe_ic_fast(
    ctx: *mut ExecCtx,
    closure: *const crate::closure::VmClosure,
    obj: VmValue,
    cs_idx: usize,
) -> VmValue {
    jit_get_property_ic_fast(ctx, closure, obj, cs_idx)
}
