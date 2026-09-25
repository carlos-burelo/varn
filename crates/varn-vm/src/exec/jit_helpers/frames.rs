//! K3-faseA: TODO este archivo entero. `jit_prepare_call` prepara la ventana
//! del callee, y `jit_push_self_frame` / `jit_post_call` son las dos mitades
//! que el prólogo/epílogo compilado invoca: todo opera sobre el layout
//! `Vec<VmValue>` contiguo, que ya no existe. Solo lo invoca código generado
//! (ausente con `FRAME_LAYOUT_V2_JIT_BAIL`), así que cada cuerpo queda
//! invalidado con tripwire. La fase B lo restaura de git. Se conservan firmas
//! y ABI para que la tabla de helpers siga enlazando.

use crate::exec::ctx::ExecCtx;
use crate::exec::jit_helpers::construct::jit_propagate_error;
use crate::value::VmValue;

macro_rules! bailed {
    () => {
        unreachable!("K3-faseA: helper de código compilado; ver FRAME_LAYOUT_V2_JIT_BAIL")
    };
}

pub(crate) extern "C" fn jit_prepare_call(
    ctx: *mut ExecCtx,
    callee: VmValue,
    callee_base: usize,
    arg_count: usize,
) -> *const crate::closure::VmClosure {
    let _ = (ctx, callee, callee_base, arg_count);
    bailed!()
}

pub(crate) extern "C" fn jit_push_self_frame(ctx: *mut ExecCtx, callee_base: usize) {
    let _ = (ctx, callee_base);
    bailed!()
}

pub(crate) extern "C" fn jit_post_call(
    ctx: *mut ExecCtx,
    callee_base: usize,
    val: VmValue,
) -> VmValue {
    let _ = (ctx, callee_base, val);
    bailed!()
}

/// Load the static function at `proto_idx` of the RUNNING closure's module as
/// a `VmValue` closure, mirroring the interpreter's `LoadStaticFn` arm: the
/// per-`proto` cache (`static_closures`) makes a second load a map hit, and the
/// new closure inherits the running module's `module_base`.
pub(crate) extern "C" fn jit_load_static_fn(
    ctx: *mut ExecCtx,
    closure: *const crate::closure::VmClosure,
    proto_idx: usize,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let closure_ref = &*closure;
        let proto = match closure_ref.proto.chunk.constants.get(proto_idx) {
            Some(varn_types::PoolEntry::Function(p)) => p,
            _ => jit_propagate_error(
                ctx_ref,
                crate::error::RuntimeError::new(format!(
                    "LoadStaticFn: const {proto_idx} is not a function"
                )),
            ),
        };
        let proto_ptr = std::rc::Rc::as_ptr(proto) as usize;
        let val = if let Some(&(_, cached)) = ctx_ref.static_closures.get(&proto_ptr) {
            cached
        } else {
            let constants = std::rc::Rc::new(crate::exec::calls::resolve_constants(
                proto,
                &mut ctx_ref.heap,
            ));
            let mut vm_closure = crate::closure::VmClosure::with_upvalues(
                proto.clone(),
                vec![],
                constants,
                ctx_ref.settings,
            );
            vm_closure.module_base = closure_ref.module_base;
            let val = ctx_ref.heap.alloc_vm_closure(std::rc::Rc::new(vm_closure));
            ctx_ref
                .static_closures
                .insert(proto_ptr, (proto.clone(), val));
            val
        };
        ctx_ref.jit_native_result = val;
    }
}
