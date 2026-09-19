//! K3-faseA: TODO este archivo entero. `jit_prepare_call` prepara la ventana
//! del callee, y `jit_push_self_frame` / `jit_post_call` son las dos mitades
//! que el prólogo/epílogo compilado invoca: todo opera sobre el layout
//! `Vec<VmValue>` contiguo, que ya no existe. Solo lo invoca código generado
//! (ausente con `FRAME_LAYOUT_V2_JIT_BAIL`), así que cada cuerpo queda
//! invalidado con tripwire. La fase B lo restaura de git. Se conservan firmas
//! y ABI para que la tabla de helpers siga enlazando.

use crate::exec::ctx::ExecCtx;
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

pub(crate) extern "C" fn jit_load_static_fn(
    ctx: *mut ExecCtx,
    closure: *const crate::closure::VmClosure,
    proto_idx: usize,
) {
    let _ = (ctx, closure, proto_idx);
    bailed!()
}
