//! Inline-cache-backed dispatch: virtual invocation and the monomorphic
//! property fast paths.
//!
//! These are the helpers that read a cache slot first and only fall back to a
//! full lookup on a miss.

use super::construct::{jit_propagate_error, resolve_constructor_return};
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;

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
                if let Some(jit_fn) = closure.jit_fn().filter(|_| is_eligible) {
                    let callee_base = base + args.arg_start;
                    let required = callee_base + closure.proto.register_count as usize + 32;
                    if ctx_ref.stack.len() < required {
                        ctx_ref.stack.resize(required, VmValue::null());
                    }
                    ctx_ref
                        .frames
                        .push(crate::frame::CallFrame::new(&**closure, callee_base));
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

/// Flat-argument shim over [`jit_invoke_virtual`] for the CLIF backend.
#[allow(clippy::too_many_arguments)]
pub(crate) extern "C" fn jit_invoke_virtual_flat(
    ctx: *mut ExecCtx,
    closure: *const crate::closure::VmClosure,
    this_val: VmValue,
    name_idx: usize,
    arg_start: usize,
    arg_count: usize,
    dest: usize,
    ip: usize,
) -> VmValue {
    let args = varn_jit::JitInvokeVirtualArgs {
        this_val,
        name_idx,
        arg_start,
        arg_count,
        dest,
        ip,
    };
    jit_invoke_virtual(ctx, closure, &args)
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
                if let Some(crate::heap::HeapObj::Object(o)) = ctx_ref.heap.get(obj.as_heap_idx())
                {
                    let guard = o.read();
                    let slot_cache = &*closure_ref.ic_cache.as_ptr();
                    let poly_slot = &slot_cache[cs_idx];
                    for entry in &poly_slot.entries {
                        if entry.id != 0 && entry.is_class == 1 {
                            if guard.shape().id == entry.id {
                                if let Some(v) = guard.field_at(entry.slot as usize) {
                                    return v;
                                }
                            }
                        }
                    }
                }
            }

            // A `.length` IC entry (8 = Array, 9 = str) at this site means the
            // interpreter already validated the semantics; the receiver's own
            // tag is the guard, so no class resolution is needed.
            let slot_cache = &*closure_ref.ic_cache.as_ptr();
            let poly_slot = &slot_cache[cs_idx];
            for entry in &poly_slot.entries {
                if entry.id != 0 && (entry.is_class == 8 || entry.is_class == 9) {
                    if let Some(v) = crate::exec::strings::fast_length(obj, &ctx_ref.heap) {
                        return v;
                    }
                    break;
                }
            }
        }
        VmValue(0x7FF8_0000_0000_0000)
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

