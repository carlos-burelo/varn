//! property fast paths.
//!
//! K3-faseA: TODO este archivo entero. Son los helpers que leen un cache slot
//! antes del lookup completo, operando sobre el layout `Vec<VmValue>`
//! contiguo. Solo los invoca código generado (ausente con
//! `FRAME_LAYOUT_V2_JIT_BAIL`): cuerpos invalidados con tripwire. La fase B
//! lo restaura de git. Se conservan firmas y ABI para que la tabla de helpers
//! siga enlazando.

use super::construct::jit_propagate_error;
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;

macro_rules! bailed {
    () => {
        unreachable!("K3-faseA: helper de código compilado; ver FRAME_LAYOUT_V2_JIT_BAIL")
    };
}

/// `InvokeVirtual`: the compiled caller flushed `this` and its args to the
/// caller activation's homes; `exec_call_method_reg` reads them (base = act_id)
/// and resolves the method through the class's virtual table
/// (`usize::MAX` cache slot = no static inline cache).
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
                    while ctx_ref.frames.len() > caller_depth {
                        let f = ctx_ref.frames.pop().unwrap();
                        ctx_ref.close_upvalues_in(f.base);
                    }
                    jit_propagate_error(ctx_ref, e);
                }
            }
            Ok(false) => {}
            Err(e) => {
                while ctx_ref.frames.len() > caller_depth {
                    let f = ctx_ref.frames.pop().unwrap();
                    ctx_ref.close_upvalues_in(f.base);
                }
                jit_propagate_error(ctx_ref, e);
            }
        }

        ctx_ref.stack.box_reg(base, args.dest)
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
