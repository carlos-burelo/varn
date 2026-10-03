use super::construct::jit_propagate_error;
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;

/// Camino dinámico ÚNICO v2 (§3.2): todo lo no-estático (métodos, closures,
/// `dynamic`) pasa por aquí. Ventana contigua ya preparada por el caller en
/// sus homes; resuelve, ejecuta (entrando a código compilado si existe) y deja
/// boxed en `ctx.jit_native_result` (el retorno directo `-> VmValue` llega con
/// la convención por target, §3.1). Un call-site monomórfico caliente se
/// recompila a estático directo; sin IC inlineado a mano por call-site.
#[varn_op_macros::jit_slow(field = "invoke_dynamic")]
pub(crate) extern "C" fn jit_invoke_dynamic(
    ctx: *mut ExecCtx,
    callee_tag: u64,
    callee_payload: u64,
    act_id: usize,
    arg_start: usize,
    argc: u32,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let callee = VmValue::from_raw_parts(callee_tag, callee_payload);
        let window = ctx_ref.stack.box_range(act_id, arg_start, argc as usize);
        match ctx_ref.invoke(callee, &window) {
            Ok(v) => ctx_ref.jit_native_result = v,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}

/// `extern "C" fn(ctx, callee_tag, callee_payload, window: *const VmValue,
/// argc)` — `new Class(...args)` out of the SSA lowering. `window[0..argc]`
/// holds the already-boxed arguments.
///
/// A class with a trivial constructor (straight-line parameter stores, or
/// none) is built inline — allocate, null the reference slots, run the
/// stores with the interpreter's own `write_field` semantics — with no
/// constructor frame pushed. Anything else runs the canonical
/// `prepare_call` path both tiers share, so errors (arity, exotic ctors,
/// non-class callees) surface exactly as interpreted.
#[varn_op_macros::jit_slow(field = "jit_new_window")]
pub(crate) extern "C" fn jit_new_window(
    ctx: *mut ExecCtx,
    callee_tag: u64,
    callee_payload: u64,
    window: *const VmValue,
    argc: usize,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let callee = VmValue::from_raw_parts(callee_tag, callee_payload);
        let caller_depth = ctx_ref.frames.len();
        // Root everything before anything can allocate or collect.
        ctx_ref.stage.clear();
        ctx_ref.stage.push(callee);
        ctx_ref
            .stage
            .extend_from_slice(std::slice::from_raw_parts(window, argc));
        if callee.is_heap() {
            if let Some(crate::heap::HeapObj::Class(cls)) = ctx_ref.heap.get(callee.as_heap_idx()) {
                let cls = cls.clone();
                match try_trivial_construct(ctx_ref, &cls, argc) {
                    Ok(Some(instance)) => {
                        ctx_ref.jit_native_result = instance;
                        return;
                    }
                    Ok(None) => {}
                    Err(e) => {
                        return fail_construct(ctx_ref, caller_depth, e);
                    }
                }
            }
        }
        let prepared = match ctx_ref.prepare_call(callee, argc + 1) {
            Ok(p) => p,
            Err(e) => return fail_construct(ctx_ref, caller_depth, e),
        };
        if let Err(e) = ctx_ref.dispatch_prepared_call(prepared) {
            return fail_construct(ctx_ref, caller_depth, e);
        }
        if ctx_ref.frames.len() > caller_depth {
            match ctx_ref.run_until(caller_depth) {
                Ok(v) => ctx_ref.jit_native_result = v,
                Err(e) => fail_construct(ctx_ref, caller_depth, e),
            }
        } else {
            ctx_ref.jit_native_result = ctx_ref.stage_pop();
        }
    }
}

/// Pop frames pushed by a failed construction, close their upvalues, and
/// unwind to the interpreter with the error — the same epilogue every
/// windowed call helper shares.
unsafe fn fail_construct(
    ctx_ref: &mut ExecCtx,
    caller_depth: usize,
    e: crate::error::RuntimeError,
) {
    while ctx_ref.frames.len() > caller_depth {
        let f = ctx_ref.frames.pop().unwrap();
        ctx_ref.close_upvalues_in(f.base);
    }
    jit_propagate_error(ctx_ref, e);
}

/// The inline half of [`jit_new_window`]: `Ok(Some)` is a fully built
/// instance, `Ok(None)` declines to the generic path, `Err` raises exactly
/// what running the trivial constructor would raise.
fn try_trivial_construct(
    ctx: &mut ExecCtx,
    cls: &std::rc::Rc<varn_types::value::ClassObj>,
    argc: usize,
) -> Result<Option<VmValue>, crate::error::RuntimeError> {
    let ctor = cls.constructor();
    let resolved = match ctor {
        None => Some((None, None)),
        Some(ctor) => ctx.heap.closure_of(ctor).and_then(|c| {
            let plan = c.proto.trivial_field_init_plan()?;
            Some((Some(plan), Some(c.proto.arity.saturating_sub(1))))
        }),
    };
    let Some((plan, arity)) = resolved else {
        return Ok(None);
    };
    let Some(plan) = plan else {
        // No constructor: the interpreter ignores the arguments.
        let inst = varn_types::value::InstanceRef::alloc(cls.clone());
        return Ok(Some(VmValue::from_heap_idx(
            ctx.heap.alloc(crate::heap::HeapObj::Instance(inst)),
        )));
    };
    let arity = arity.unwrap_or(0);
    if argc != arity {
        return Ok(None);
    }
    let inst = varn_types::value::InstanceRef::alloc(cls.clone());
    let instance_nv =
        VmValue::from_heap_idx(ctx.heap.alloc(crate::heap::HeapObj::Instance(inst.clone())));
    for (param_idx, offset, tag) in plan.iter() {
        let Some(&arg) = ctx.stage.get(1 + *param_idx) else {
            return Ok(None);
        };
        inst.write_field_at(*offset, *tag, arg)
            .map_err(crate::error::RuntimeError::new)?;
        ctx.heap.write_barrier(instance_nv.as_heap_idx(), arg);
    }
    Ok(Some(instance_nv))
}
