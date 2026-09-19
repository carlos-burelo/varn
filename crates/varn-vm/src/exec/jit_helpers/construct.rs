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
    let handler = ctx
        .jit_panic_exception_handler
        .take()
        .or_else(|| ctx.try_handlers.pop());
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
    // K3-faseA: solo lo invocaba código generado (fase B: restaurar de git).
    let _ = (ctx_ref, cls, base, args);
    unreachable!("K3-faseA: helper de código compilado; ver FRAME_LAYOUT_V2_JIT_BAIL");
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

/// `new cls(...)` con los argumentos ya preparados en `staging`, cuyo primer
/// slot es el placeholder de callee que la instancia reemplaza. Devuelve
/// `None` en todo lo que el camino rápido no cubre (ctor async/generador/rest,
/// ctor nativo, sin plan trivial) — el llamante toma entonces la vía genérica
/// `prepare_call`.
///
/// Sin esto, cada `new X()` pagaba `prepare_call` + frame + `run_until`
/// anidado. El camino JIT2JIT directo (llamar al `jit_entry` del ctor) está
/// cortado en la fase A del frame por clases (`FRAME_LAYOUT_V2_JIT_BAIL`):
/// se restaura en la fase B.
pub(crate) fn construct_staged_fast(
    ctx_ref: &mut ExecCtx,
    cls: &std::rc::Rc<varn_types::ClassObj>,
    staging: &[VmValue],
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

    let (ctor_closure, jit_fn) = match cached_entry {
        Some((closure, jit_fn)) => (Some(closure), jit_fn),
        None => {
            let cached: Option<Option<std::rc::Rc<dyn std::any::Any>>> =
                match &*cls.ctor_rt_cache.borrow() {
                    Some((cached_ver, entry)) if *cached_ver == ver => Some(entry.clone()),
                    _ => None,
                };
            let closure_opt = match cached {
                Some(None) => None,
                Some(Some(any)) => any.downcast::<crate::closure::VmClosure>().ok(),
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
                            *cls.ctor_rt_cache.borrow_mut() =
                                Some((ver, Some(nc.clone() as std::rc::Rc<dyn std::any::Any>)));
                            Some(nc.clone())
                        }
                        Some(_) => return None,
                        None => {
                            *cls.ctor_rt_cache.borrow_mut() = Some((ver, None));
                            None
                        }
                    }
                }
            };
            if let Some(ref nc) = closure_opt {
                let jit_fn = nc.hot_jit_fn();
                ACTIVE_CTOR.with(|cell| {
                    *cell.borrow_mut() = Some(ActiveCtorCache {
                        class_id: cls.id,
                        version: ver,
                        closure: nc.clone(),
                        jit_fn,
                    });
                });
                (Some(nc.clone()), jit_fn)
            } else {
                (None, None)
            }
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
    if let Some(ref closure) = ctor_closure {
        if let Some(plan) = closure.proto.trivial_field_init_plan() {
            // Fast inlining: directly assign arguments into object slots
            // Arguments are staged at `staging[1 + param_idx]`.
            for &(param_idx, slot) in &*plan {
                if let Some(&val) = staging.get(1 + param_idx) {
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

    // Sin plan trivial no hay fast path: el llamante va por `prepare_call`.
    // (El resto original —invocar el `jit_entry` del ctor directamente—
    // exige código compilado contra el layout antiguo: cortado en la fase A,
    // se restaura de git en la fase B.)
    let _ = (ctor_closure, jit_fn);
    None
}

pub(crate) extern "C" fn jit_alloc_instance_fast(
    ctx: *mut ExecCtx,
    class_id: u32,
    payload_size: u32,
) -> u64 {
    unsafe {
        let ctx_ref = &mut *ctx;
        let inst = varn_types::value::InstanceRef::alloc_with_layout(class_id, payload_size);
        ctx_ref.heap.alloc(crate::heap::HeapObj::Instance(inst)) as u64
    }
}
