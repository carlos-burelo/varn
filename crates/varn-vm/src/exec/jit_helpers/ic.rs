//! Inline-cache-backed dispatch: virtual invocation and the monomorphic
//! property fast paths.
//!
//! These are the helpers that read a cache slot first and only fall back to a
//! full lookup on a miss.

use super::construct::jit_propagate_error;
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;
use varn_types::chunk::ICKind;

const METHOD_CACHE_SIZE: usize = 64;

struct ActiveMethodCache {
    caller_proto: usize,
    class_id: u32,
    name_idx: usize,
    closure: std::rc::Rc<crate::closure::VmClosure>,
    jit_fn: Option<varn_jit::JitFn>,
}

thread_local! {
    static ACTIVE_METHODS: std::cell::RefCell<[Option<ActiveMethodCache>; METHOD_CACHE_SIZE]> = const {
        std::cell::RefCell::new([const { None }; METHOD_CACHE_SIZE])
    };
}

#[inline(always)]
pub(crate) fn try_fast_jit_method(
    ctx_ref: &mut ExecCtx,
    closure_ref: &crate::closure::VmClosure,
    this_val: VmValue,
    base: usize,
    name_idx: usize,
    arg_start: usize,
    arg_count: usize,
) -> Option<VmValue> {
    if !this_val.is_heap() {
        return None;
    }
    let class_rc = match ctx_ref.heap.get(this_val.as_heap_idx()) {
        Some(crate::heap::HeapObj::Object(o) | crate::heap::HeapObj::Record(o)) => {
            o.borrow().class()?
        }
        _ => return None,
    };

    let caller_proto = &closure_ref.proto as *const _ as usize;
    let slot_idx = (class_rc.id as usize ^ name_idx ^ (caller_proto >> 4)) & (METHOD_CACHE_SIZE - 1);
    let hit = ACTIVE_METHODS.with(|cell| {
        let mut b = cell.borrow_mut();
        if let Some(ref mut c) = b[slot_idx] {
            if c.caller_proto == caller_proto
                && c.class_id == class_rc.id
                && c.name_idx == name_idx
            {
                if c.jit_fn.is_none() {
                    c.jit_fn = c.closure.hot_jit_fn();
                }
                return Some((c.closure.clone(), c.jit_fn));
            }
        }
        None
    });

    let (nc, jit_fn) = match hit {
        Some(pair) => pair,
        None => {
            let name_nv = *closure_ref.constants.get(name_idx)?;
            let name = ctx_ref.heap.str_val(name_nv)?;
            let (method_val, _owner) =
                varn_types::find_method_with_owner(&class_rc, name.as_ref())?;
            let nc = match method_val {
                varn_types::Value::VmValue(payload) => {
                    let wrapper = payload
                        .as_any()
                        .downcast_ref::<crate::closure::VmClosurePayload>()?;
                    wrapper.0.clone()
                }
                _ => return None,
            };
            if nc.proto.is_generator
                || nc.proto.is_async
                || arg_count > nc.proto.arity
                || nc.proto.has_try()
            {
                return None;
            }
            let jit_fn = nc.hot_jit_fn();
            ACTIVE_METHODS.with(|cell| {
                cell.borrow_mut()[slot_idx] = Some(ActiveMethodCache {
                    caller_proto,
                    class_id: class_rc.id,
                    name_idx,
                    closure: nc.clone(),
                    jit_fn,
                });
            });
            (nc, jit_fn)
        }
    };

    let jit_fn = jit_fn?;

    let orig_len = ctx_ref.stack.len();
    let callee_base = orig_len;
    let required = callee_base + nc.proto.register_count as usize + 32;
    if ctx_ref.stack.len() < required {
        ctx_ref.stack.resize(required, VmValue::null());
    }
    ctx_ref.stack[callee_base] = this_val;
    let src_start = base + arg_start;
    for i in 0..arg_count {
        ctx_ref.stack[callee_base + 1 + i] = ctx_ref.stack[src_start + i];
    }
    for i in (1 + arg_count)..(nc.proto.arity) {
        ctx_ref.stack[callee_base + i] = VmValue::null();
    }
    ctx_ref
        .frames
        .push(crate::frame::CallFrame::new(&nc, callee_base));
    ctx_ref.jit_frame_prepushed = 1;
    let res = unsafe {
        (jit_fn)(
            ctx_ref.stack.as_mut_ptr() as *mut std::ffi::c_void,
            &*nc as *const crate::closure::VmClosure as *const std::ffi::c_void,
            callee_base,
            ctx_ref as *mut ExecCtx as *mut std::ffi::c_void,
        )
    };
    ctx_ref.frames.pop();
    if nc.proto.upvalue_count > 0 {
        ctx_ref.close_upvalues_above(callee_base);
    }
    ctx_ref.stack.truncate(orig_len);
    Some(res)
}

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

        if let Some(res) = try_fast_jit_method(
            ctx_ref,
            closure_ref,
            args.this_val,
            base,
            args.name_idx,
            args.arg_start,
            args.arg_count,
        ) {
            ctx_ref.stack[base + args.dest] = res;
            return res;
        }

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
