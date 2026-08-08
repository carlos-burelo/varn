//! Indexed access and the array surface.
//!
//! `jit_array_get_fast` / `jit_array_set_fast` are the guarded fast paths:
//! they check the representation discriminant, then index without boxing.

use super::construct::jit_propagate_error;
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;

pub(crate) extern "C" fn jit_get_index(
    ctx: *mut ExecCtx,
    args: *const varn_jit::JitGetIndexArgs,
) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let args = &*args;
        match crate::exec::collections::get_index(args.obj, args.key, &mut ctx_ref.heap) {
            Ok(v) => v,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

pub(crate) extern "C" fn jit_set_index(ctx: *mut ExecCtx, args: *const varn_jit::JitSetIndexArgs) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let args = &*args;
        match crate::exec::collections::set_index(args.obj, args.key, args.val, &mut ctx_ref.heap) {
            Ok(()) => {}
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

pub(crate) unsafe extern "C" fn jit_array_get_fast(
    ctx: *mut ExecCtx,
    obj: VmValue,
    key: VmValue,
) -> VmValue {
    // Fast path: heap array with integer key — no string allocation, no dispatch table
    if obj.is_heap() {
        let heap_idx = obj.as_heap_idx();
        let ctx_ref = &*ctx;
        if let Some(crate::heap::HeapObj::Array(a)) = ctx_ref.heap.get(heap_idx) {
            let idx = if key.is_int() {
                key.as_int() as usize
            } else {
                key.to_i32() as usize
            };
            return a.get_vm(idx).unwrap_or(VmValue::null());
        }
    }
    // Slow path: objects, strings, ranges, SSO strings — fall back to handler
    let ctx_ref = &mut *ctx;
    match crate::exec::collections::array_get_index(obj, key, &mut ctx_ref.heap) {
        Ok(v) => v,
        Err(e) => super::jit_propagate_error(ctx_ref, e),
    }
}

pub(crate) unsafe extern "C" fn jit_array_set_fast(
    ctx: *mut ExecCtx,
    obj: VmValue,
    key: VmValue,
    val: VmValue,
) {
    // Fast path: heap array with integer key — no string allocation, no dispatch
    if obj.is_heap() {
        let heap_idx = obj.as_heap_idx();
        let ctx_ref = &mut *ctx;
        if let Some(crate::heap::HeapObj::Array(a)) = ctx_ref.heap.get_mut(heap_idx) {
            let idx = if key.is_int() {
                key.as_int() as usize
            } else {
                key.to_i32() as usize
            };
            let len = a.len();
            if idx < len {
                a.set_vm(idx, val);
            } else if idx == len {
                a.push_vm(val);
            } else {
                while a.len() < idx {
                    a.push_vm(VmValue::null());
                }
                a.push_vm(val);
            }
            ctx_ref.heap.write_barrier(heap_idx, val);
            return;
        }
    }
    // Slow path: objects and other types
    let ctx_ref = &mut *ctx;
    if let Err(e) = crate::exec::collections::array_set_index(obj, key, val, &mut ctx_ref.heap) {
        super::jit_propagate_error(ctx_ref, e);
    }
}

pub(crate) extern "C" fn jit_array_length(ctx: *mut ExecCtx, arr: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        match ctx_ref.exec_array_length(arr) {
            Ok(v) => v,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

pub(crate) extern "C" fn jit_array_push(ctx: *mut ExecCtx, arr: VmValue, val: VmValue) {
    unsafe {
        let ctx_ref = &mut *ctx;
        match ctx_ref.exec_array_push(arr, val) {
            Ok(()) => {}
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

pub(crate) extern "C" fn jit_array_pop(ctx: *mut ExecCtx, arr: VmValue) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        match ctx_ref.exec_array_pop(arr) {
            Ok(v) => v,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

pub(crate) extern "C" fn jit_array_extend(ctx: *mut ExecCtx, arr: VmValue, src: VmValue) {
    unsafe {
        let ctx_ref = &mut *ctx;
        match ctx_ref.exec_array_extend(arr, src) {
            Ok(()) => {}
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

