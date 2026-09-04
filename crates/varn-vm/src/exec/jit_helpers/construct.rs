//! Object construction reached from compiled code, and the error escape.
//!
//! `new X(...)` is the one call shape that must allocate before it can call,
//! so it does not fit the ordinary call helpers. `jit_propagate_error` sits
//! alongside it because it is the exit every helper in this tree takes when
//! it cannot return normally.

use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;

#[inline(always)]
pub(crate) unsafe fn jit_propagate_error(ctx: &mut ExecCtx, e: crate::error::RuntimeError) -> ! {
    let handler = ctx.jit_panic_exception_handler.take().or_else(|| ctx.try_handlers.pop());
    ctx.jit_panic_exception_handler = handler;
    ctx.jit_panic_exception_error = Some(e.thrown.unwrap_or(VmValue::null()));
    ctx.jit_panic_exception_err_obj = Some(e);
    let buf = ctx.jit_jmp_buf;
    if !buf.is_null() {
        crate::exec::ctx::my_longjmp(buf, 1);
    }
    panic!("JIT error: no jump buffer");
}

/// Fast `new Class(...)` from JIT code: allocates the instance and invokes a
/// JIT-compiled constructor directly (JIT2JIT), skipping the interpreter
/// prepare_call path. Returns None to fall back to the slow path (native
/// ctors, rest params, async/generator ctors, or no JIT entry).
#[inline]
pub(super) fn jit_construct_fast(
    ctx_ref: &mut ExecCtx,
    cls: &std::rc::Rc<varn_types::ClassObj>,
    base: usize,
    args: &varn_jit::JitCallArgs,
) -> Option<VmValue> {
    let v = construct_staged_fast(ctx_ref, cls, base + args.arg_start)?;
    ctx_ref.stack[base + args.dest] = v;
    ctx_ref.record_call_vm_fast();
    Some(v)
}

struct ActiveCtorCache {
    class_id: u32,
    version: u32,
    closure: std::rc::Rc<crate::closure::VmClosure>,
    jit_fn: Option<varn_jit::JitFn>,
}

thread_local! {
    static ACTIVE_CTOR: std::cell::RefCell<Option<ActiveCtorCache>> = const { std::cell::RefCell::new(None) };
}

/// `new cls(...)` with the arguments already staged as a contiguous window at
/// `callee_base`, whose first slot is the callee placeholder the instance
/// replaces. Returns `None` for any shape the fast path does not cover (async /
/// generator / rest constructor, a constructor that is not clif-compiled, a
/// native constructor) — the caller must then take the generic
/// `prepare_call` path.
///
/// Without this, every `new X()` reaching clif code goes through
/// `prepare_call` + a frame push + a nested `run_until`: 10k of the 10.3k slow
/// calls in tests/main.vn, worth ~3.5x on that suite.
pub(crate) fn construct_staged_fast(
    ctx_ref: &mut ExecCtx,
    cls: &std::rc::Rc<varn_types::ClassObj>,
    callee_base: usize,
) -> Option<VmValue> {
    use crate::alloc_profile as prof;
    let on = prof::enabled();
    let t_ctor = if on { prof::read() } else { 0 };
    let ver = cls
        .vtable_version
        .load(std::sync::atomic::Ordering::Relaxed);
    let cached_entry = ACTIVE_CTOR.with(|cell| {
        let mut b = cell.borrow_mut();
        if let Some(ref mut c) = *b {
            if c.class_id == cls.id && c.version == ver {
                if c.jit_fn.is_none() {
                    c.jit_fn = c.closure.hot_jit_fn();
                }
                return Some((c.closure.clone(), c.jit_fn));
            }
        }
        None
    });

    let jit_ctor = match cached_entry {
        Some((closure, jit_fn)) => jit_fn.map(|f| (closure, f)),
        None => {
            let cached: Option<Option<std::rc::Rc<dyn std::any::Any>>> =
                match &*cls.ctor_rt_cache.borrow() {
                    Some((cached_ver, entry)) if *cached_ver == ver => Some(entry.clone()),
                    _ => None,
                };
            let resolved = match cached {
                Some(None) => None,
                Some(Some(any)) => {
                    let nc = any.downcast::<crate::closure::VmClosure>().ok()?;
                    let jit_fn = nc.hot_jit_fn()?;
                    Some((nc, jit_fn))
                }
                None => {
                    let ctor = cls.constructor();
                    match &ctor {
                        Some(varn_types::Value::VmValue(payload)) => {
                            let wrapper = payload
                                .as_any()
                                .downcast_ref::<crate::closure::VmClosurePayload>()?;
                            let nc = &wrapper.0;
                            if nc.proto.is_async || nc.proto.is_generator || nc.proto.has_rest {
                                return None;
                            }
                            let jit_fn = nc.hot_jit_fn()?;
                            *cls.ctor_rt_cache.borrow_mut() =
                                Some((ver, Some(nc.clone() as std::rc::Rc<dyn std::any::Any>)));
                            Some((nc.clone(), jit_fn))
                        }
                        Some(_) => return None,
                        None => {
                            *cls.ctor_rt_cache.borrow_mut() = Some((ver, None));
                            None
                        }
                    }
                }
            };
            if let Some((ref nc, jit_fn)) = resolved {
                ACTIVE_CTOR.with(|cell| {
                    *cell.borrow_mut() = Some(ActiveCtorCache {
                        class_id: cls.id,
                        version: ver,
                        closure: nc.clone(),
                        jit_fn: Some(jit_fn),
                    });
                });
            }
            resolved
        }
    };

    if on {
        prof::record(prof::Seg::CtorResolve, t_ctor, prof::read());
    }

    let t_alloc = if on { prof::read() } else { 0 };
    let inst = varn_types::value::InstanceRef::alloc(cls.clone());
    if on {
        prof::record(prof::Seg::ObjDataAlloc, t_alloc, prof::read());
    }
    if let Some((ref closure, _)) = jit_ctor {
        if let Some(plan) = closure.proto.trivial_field_init_plan() {
            // Fast inlining: directly assign arguments into object slots
            // Arguments are staged at `callee_base + 1 + param_idx`.
            for (param_idx, slot) in plan {
                let arg_idx = callee_base + 1 + param_idx;
                if arg_idx < ctx_ref.stack.len() {
                    let val = ctx_ref.stack[arg_idx];
                    inst.set_field_at(slot, val);
                }
            }
            let t_push = if on { prof::read() } else { 0 };
            let instance_nv =
                VmValue::from_heap_idx(ctx_ref.heap.alloc(crate::heap::HeapObj::Instance(inst)));
            if on {
                prof::record(prof::Seg::HeapPush, t_push, prof::read());
            }
            return Some(instance_nv);
        }
    }

    let t_push = if on { prof::read() } else { 0 };
    let instance_nv =
        VmValue::from_heap_idx(ctx_ref.heap.alloc(crate::heap::HeapObj::Instance(inst)));
    if on {
        prof::record(prof::Seg::HeapPush, t_push, prof::read());
    }
    let t_frame = if on { prof::read() } else { 0 };

    let Some((closure, jit_fn)) = jit_ctor else {
        return Some(instance_nv);
    };

    let required = callee_base + closure.proto.register_count as usize + 32;
    if ctx_ref.stack.len() < required {
        ctx_ref.stack.resize(required, VmValue::null());
    }
    ctx_ref.stack[callee_base] = instance_nv;

    let mut frame = crate::frame::CallFrame::new(&closure, callee_base);
    frame.current_class = Some(cls.clone());
    ctx_ref.frames.push(frame);

    ctx_ref.jit_frame_prepushed = 1;
    let res = unsafe {
        (jit_fn)(
            ctx_ref.stack.as_mut_ptr() as *mut std::ffi::c_void,
            &*closure as *const crate::closure::VmClosure as *const std::ffi::c_void,
            callee_base,
            ctx_ref as *mut ExecCtx as *mut std::ffi::c_void,
        )
    };

    let final_instance = ctx_ref.stack[callee_base];
    ctx_ref.frames.pop();
    if closure.proto.upvalue_count > 0 {
        ctx_ref.close_upvalues_above(callee_base);
    }
    if on {
        prof::record(prof::Seg::CtorFrame, t_frame, prof::read());
    }

    Some(if res.is_null() { final_instance } else { res })
}
