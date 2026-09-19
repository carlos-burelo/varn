//! property fast paths.
//!
//! K3-faseA: TODO este archivo entero. Son los helpers que leen un cache slot
//! antes del lookup completo, operando sobre el layout `Vec<VmValue>`
//! contiguo. Solo los invoca código generado (ausente con
//! `FRAME_LAYOUT_V2_JIT_BAIL`): cuerpos invalidados con tripwire. La fase B
//! lo restaura de git. Se conservan firmas y ABI para que la tabla de helpers
//! siga enlazando.

use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;

macro_rules! bailed {
    () => {
        unreachable!("K3-faseA: helper de código compilado; ver FRAME_LAYOUT_V2_JIT_BAIL")
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
    let _ = (
        ctx_ref,
        closure_ref,
        this_val,
        base,
        name_idx,
        arg_start,
        arg_count,
    );
    bailed!()
}

pub(crate) extern "C" fn jit_invoke_virtual(
    ctx: *mut ExecCtx,
    closure: *const crate::closure::VmClosure,
    args: *const varn_jit::JitInvokeVirtualArgs,
) -> VmValue {
    let _ = (ctx, closure, args);
    bailed!()
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
    let _ = (
        ctx,
        closure,
        this_tag,
        this_payload,
        name_idx,
        arg_start,
        arg_count,
        dest,
        ip,
    );
    bailed!()
}

pub(crate) extern "C" fn jit_get_property_ic_fast(
    ctx: *mut ExecCtx,
    closure: *const crate::closure::VmClosure,
    obj: VmValue,
    cs_idx: usize,
) -> VmValue {
    let _ = (ctx, closure, obj, cs_idx);
    bailed!()
}

pub(crate) extern "C" fn jit_get_property_maybe_ic_fast(
    ctx: *mut ExecCtx,
    closure: *const crate::closure::VmClosure,
    obj: VmValue,
    cs_idx: usize,
) -> VmValue {
    let _ = (ctx, closure, obj, cs_idx);
    bailed!()
}
