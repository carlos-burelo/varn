//! Indexed access and the array surface.
//!
//! `jit_array_get_fast` / `jit_array_set_fast` are the guarded fast paths:
//! they check the representation discriminant, then index without boxing.

use super::construct::jit_propagate_error;
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;

pub(crate) extern "C" fn jit_get_index(ctx: *mut ExecCtx, args: *const varn_jit::JitGetIndexArgs) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let args = &*args;
        match crate::exec::collections::get_index(args.obj, args.key, &mut ctx_ref.heap) {
            Ok(v) => ctx_ref.jit_native_result = v,
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
    obj_tag: u64,
    obj_payload: u64,
    key_tag: u64,
    key_payload: u64,
) {
    let obj = VmValue::from_raw_parts(obj_tag, obj_payload);
    let key = VmValue::from_raw_parts(key_tag, key_payload);
    // Fast path: heap array or object
    if obj.is_heap() {
        let heap_idx = obj.as_heap_idx();
        let ctx_ref = &*ctx;
        match ctx_ref.heap.get(heap_idx) {
            Some(crate::heap::HeapObj::Array(a)) => {
                let idx = if key.is_int() {
                    key.as_int() as usize
                } else {
                    key.to_i32() as usize
                };
                (*ctx).jit_native_result = a.get_vm(idx).unwrap_or(VmValue::null());
                return;
            }
            Some(crate::heap::HeapObj::Object(o) | crate::heap::HeapObj::Record(o)) => {
                let mut buf = [0u8; 5];
                let key_str = if key.is_sso() {
                    Some(key.sso_as_str(&mut buf))
                } else if key.is_heap() {
                    match ctx_ref.heap.get(key.as_heap_idx()) {
                        Some(crate::heap::HeapObj::Str(s)) => Some(s.as_str()),
                        _ => None,
                    }
                } else {
                    None
                };
                if let Some(ks) = key_str {
                    (*ctx).jit_native_result = o.read().get(ks).unwrap_or(VmValue::null());
                    return;
                }
            }
            Some(crate::heap::HeapObj::Map(m)) => {
                let found = ctx_ref
                    .heap
                    .lookup_map_key(key)
                    .and_then(|k| m.borrow().get(&k).copied());
                (*ctx).jit_native_result = found.unwrap_or_else(VmValue::null);
                return;
            }
            _ => {}
        }
    }
    // Slow path: strings, ranges, SSO strings — fall back to handler
    let ctx_ref = &mut *ctx;
    match crate::exec::collections::array_get_index(obj, key, &mut ctx_ref.heap) {
        Ok(v) => ctx_ref.jit_native_result = v,
        Err(e) => super::construct::jit_propagate_error(ctx_ref, e),
    }
}

pub(crate) unsafe extern "C" fn jit_array_set_fast(
    ctx: *mut ExecCtx,
    obj_tag: u64,
    obj_payload: u64,
    key_tag: u64,
    key_payload: u64,
    val_tag: u64,
    val_payload: u64,
) {
    let obj = VmValue::from_raw_parts(obj_tag, obj_payload);
    let key = VmValue::from_raw_parts(key_tag, key_payload);
    let val = VmValue::from_raw_parts(val_tag, val_payload);
    // Fast path: heap array or object or map
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
        if let Some(crate::heap::HeapObj::Map(m)) = ctx_ref.heap.get_mut(heap_idx) {
            let m = if std::rc::Rc::strong_count(&m.0) > 1 {
                let cloned = m.borrow().clone();
                *m = varn_types::value::MapRef::new(cloned);
                m.clone()
            } else {
                m.clone()
            };
            let k = ctx_ref.heap.canonical_map_key(key);
            m.borrow_mut().insert(k, val);
            ctx_ref.heap.write_barrier(heap_idx, val);
            return;
        }
        if let Some(crate::heap::HeapObj::Object(o)) = ctx_ref.heap.get(heap_idx) {
            let o = o.clone();
            let mut buf = [0u8; 5];
            let key_str = if key.is_sso() {
                Some(key.sso_as_str(&mut buf))
            } else if key.is_heap() {
                match ctx_ref.heap.get(key.as_heap_idx()) {
                    Some(crate::heap::HeapObj::Str(s)) => Some(s.as_str()),
                    _ => None,
                }
            } else {
                None
            };
            if let Some(ks) = key_str {
                o.set_field_str(ks, val);
                ctx_ref.heap.write_barrier(heap_idx, val);
                return;
            }
        }
    }
    // Slow path: objects and other types
    let ctx_ref = &mut *ctx;
    if let Err(e) = crate::exec::collections::array_set_index(obj, key, val, &mut ctx_ref.heap) {
        super::construct::jit_propagate_error(ctx_ref, e);
    }
}

pub(crate) extern "C" fn jit_array_length(ctx: *mut ExecCtx, arr_tag: u64, arr_payload: u64) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let arr = VmValue::from_raw_parts(arr_tag, arr_payload);
        match ctx_ref.exec_array_length(arr) {
            Ok(v) => ctx_ref.jit_native_result = v,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

pub(crate) extern "C" fn jit_array_push(
    ctx: *mut ExecCtx,
    arr_tag: u64,
    arr_payload: u64,
    val_tag: u64,
    val_payload: u64,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let arr = VmValue::from_raw_parts(arr_tag, arr_payload);
        let val = VmValue::from_raw_parts(val_tag, val_payload);
        match ctx_ref.exec_array_push(arr, val) {
            Ok(()) => {}
            Err(e) => {
                eprintln!("ARRAY_PUSH ERR: arr={:?} (tag={arr_tag}, payload={arr_payload:#x}) val={:?} err={:?}", arr, val, e);
                jit_propagate_error(ctx_ref, e);
            }
        }
    }
}

pub(crate) extern "C" fn jit_array_pop(ctx: *mut ExecCtx, arr_tag: u64, arr_payload: u64) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let arr = VmValue::from_raw_parts(arr_tag, arr_payload);
        match ctx_ref.exec_array_pop(arr) {
            Ok(v) => ctx_ref.jit_native_result = v,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

pub(crate) extern "C" fn jit_array_extend(
    ctx: *mut ExecCtx,
    arr_tag: u64,
    arr_payload: u64,
    src_tag: u64,
    src_payload: u64,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let arr = VmValue::from_raw_parts(arr_tag, arr_payload);
        let src = VmValue::from_raw_parts(src_tag, src_payload);
        match ctx_ref.exec_array_extend(arr, src) {
            Ok(()) => {}
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}
