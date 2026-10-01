//! Aggregate construction from compiled code: array and string literals.

use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;

#[varn_op_macros::jit_slow(field = "build_str")]
pub(crate) extern "C" fn jit_build_str(ctx: *mut ExecCtx, parts_ptr: *const VmValue, count: usize) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let parts = std::slice::from_raw_parts(parts_ptr, count);
        let mut out = crate::strbuf::StrBuf::new();
        for &v in parts {
            ctx_ref.heap.str_repr_into(v, &mut out);
        }
        ctx_ref.jit_native_result = ctx_ref.heap.alloc_str_dynamic(out.as_str());
    }
}

/// `extern "C" fn(*mut ExecCtx, parts: *const VmValue, count)` — build an array
/// from a boxed window staged on the caller's native stack.
#[varn_op_macros::jit_slow(field = "build_array_window")]
pub(crate) extern "C" fn jit_build_array_window(
    ctx: *mut ExecCtx,
    parts_ptr: *const VmValue,
    count: usize,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let parts = std::slice::from_raw_parts(parts_ptr, count);
        ctx_ref.jit_native_result = ctx_ref.heap.alloc_array_vm(parts.to_vec());
    }
}

/// Empty object for spread literals (`{...a}` with no keyed prefix): the SSA
/// lowering then sets/merges each part through the property helpers.
#[varn_op_macros::jit_slow(field = "build_empty_object")]
pub(crate) extern "C" fn jit_build_empty_object(ctx: *mut ExecCtx) {
    unsafe {
        let ctx_ref = &mut *ctx;
        ctx_ref.jit_native_result = ctx_ref.heap.alloc_object();
    }
}

/// `extern "C" fn(*mut ExecCtx, pairs: *const VmValue, count)` — build a map
/// from a boxed `[k0, v0, k1, v1, …]` window on the caller's native stack.
#[varn_op_macros::jit_slow(field = "build_map_window")]
pub(crate) extern "C" fn jit_build_map_window(
    ctx: *mut ExecCtx,
    pairs_ptr: *const VmValue,
    count: usize,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        if count == 0 {
            ctx_ref.jit_native_result = ctx_ref.heap.alloc_empty_map_vm();
            return;
        }
        let parts = std::slice::from_raw_parts(pairs_ptr, count * 2);
        let mut map = varn_types::value::ValueMap::default();
        for i in 0..count {
            let key = ctx_ref.heap.canonical_map_key(parts[i * 2]);
            map.insert(key, parts[i * 2 + 1]);
        }
        ctx_ref.jit_native_result = ctx_ref.heap.alloc_map_vm(map);
    }
}

#[varn_op_macros::jit_slow(field = "build_object_with_shape")]
pub(crate) extern "C" fn jit_build_object_with_shape(
    ctx: *mut ExecCtx,
    base: usize,
    start_reg: usize,
    shape: *const varn_types::Shape,
    may_hold_closure: usize,
) {
    unsafe {
        use crate::alloc_profile as prof;
        let on = prof::enabled();
        let t_all = if on { prof::read() } else { 0 };
        let out = build_shaped_from_ptr(ctx, base, start_reg, shape, may_hold_closure != 0, false);
        if on {
            prof::record(prof::Seg::HelperTotal, t_all, prof::read());
        }
        (*ctx).jit_native_result = out;
    }
}

#[varn_op_macros::jit_slow(field = "build_record_with_shape")]
pub(crate) extern "C" fn jit_build_record_with_shape(
    ctx: *mut ExecCtx,
    base: usize,
    start_reg: usize,
    shape: *const varn_types::Shape,
    may_hold_closure: usize,
) {
    unsafe {
        let out = build_shaped_from_ptr(ctx, base, start_reg, shape, may_hold_closure != 0, true);
        (*ctx).jit_native_result = out;
    }
}

#[inline(always)]
unsafe fn build_shaped_from_ptr(
    ctx: *mut ExecCtx,
    base: usize,
    start_reg: usize,
    shape: *const varn_types::Shape,
    may_hold_closure: bool,
    is_record: bool,
) -> VmValue {
    let ctx_ref = &mut *ctx;
    let shape = std::mem::ManuallyDrop::new(std::rc::Rc::from_raw(shape));
    crate::exec::collections::build_with_shape(
        &ctx_ref.stack,
        base,
        start_reg,
        (*shape).clone(),
        &mut ctx_ref.heap,
        may_hold_closure,
        is_record,
    )
}

/// `extern "C" fn(*mut ExecCtx, vals: *const VmValue, count, shape: *const Shape,
/// is_record, may_hold_closure)` — build an object/record from a boxed window on
/// the caller's native stack. The window-taking sibling of
/// `jit_build_object_with_shape` (which reads homes).
#[varn_op_macros::jit_slow(field = "build_object_window")]
pub(crate) extern "C" fn jit_build_object_window(
    ctx: *mut ExecCtx,
    vals_ptr: *const VmValue,
    count: usize,
    shape: *const varn_types::Shape,
    is_record: usize,
    may_hold_closure: usize,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let vals = std::slice::from_raw_parts(vals_ptr, count);
        let shape = std::mem::ManuallyDrop::new(std::rc::Rc::from_raw(shape));
        let out = crate::exec::collections::build_with_shape_slice(
            &ctx_ref.stack,
            (*shape).clone(),
            vals,
            &mut ctx_ref.heap,
            may_hold_closure != 0,
            is_record != 0,
        );
        ctx_ref.jit_native_result = out;
    }
}

#[varn_op_macros::jit_slow(field = "range")]
pub(crate) extern "C" fn jit_range(
    ctx: *mut ExecCtx,
    start_tag: u64,
    start_payload: u64,
    end_tag: u64,
    end_payload: u64,
    flag: usize,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let start_val = VmValue::from_raw_parts(start_tag, start_payload);
        let end_val = VmValue::from_raw_parts(end_tag, end_payload);
        let mut temp = vec![start_val, end_val];
        match crate::exec::advanced::invoke_runtime_static(
            "__range__",
            &mut temp,
            &mut ctx_ref.heap,
            flag as u16,
        ) {
            Ok(v) => ctx_ref.jit_native_result = v,
            Err(e) => panic!("JIT range failed: {:?}", e),
        }
    }
}

#[varn_op_macros::jit_slow(field = "wrap_spread")]
pub(crate) extern "C" fn jit_wrap_spread(ctx: *mut ExecCtx, val_tag: u64, val_payload: u64) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let val = VmValue::from_raw_parts(val_tag, val_payload);
        let extracted = ctx_ref.heap.extract(val);
        ctx_ref.jit_native_result = ctx_ref
            .heap
            .intern(varn_types::Value::Spread(Box::new(extracted)));
    }
}
