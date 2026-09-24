//! Calling native (Rust) functions from compiled code, plus the stack-growth
//! helper their argument windows depend on.
//!
//! K3-faseA: los helpers que direccionaban la ventana contigua quedan
//! invalidados (solo los invocaba código generado). `jit_is_native_fn` no
//! toca el layout y sigue real. Fase B: restaurar de git.

use super::construct::jit_propagate_error;
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;

macro_rules! bailed {
    () => {
        unreachable!("K3-faseA: helper de código compilado; ver FRAME_LAYOUT_V2_JIT_BAIL")
    };
}

pub(crate) extern "C" fn jit_ensure_stack_capacity(ctx: *mut ExecCtx, required_len: usize) {
    let _ = (ctx, required_len);
    bailed!()
}

pub(crate) extern "C" fn jit_is_native_fn(ctx: *mut ExecCtx, callee: VmValue) -> usize {
    unsafe {
        let ctx_ref = &*ctx;
        if callee.is_heap() {
            if let Some(crate::heap::HeapObj::NativeFn(..)) = ctx_ref.heap.get(callee.as_heap_idx())
            {
                return 1;
            }
        }
        0
    }
}

/// THE one helper for a native `CallNativeOp`. The compiled caller flushed
/// `[receiver, args...]` to the home slots of registers
/// `reg_start..reg_start + total` in activation `act_id`; this boxes that
/// window through `FrameStore` and invokes the native via the SAME
/// `invoke_native` marshal the interpreter's `CallNativeOp` arm uses
/// (`call_native_with_receiver`).
///
/// `fn_addr == 0` means the target was not resolved at JIT-compile time —
/// resolve it from `op_id` here (an unknown op-id raises a VM error). One
/// entry replaces the previous `jit_call_native_op` / `jit_call_native_fnptr`
/// pair: both read the same window and only differed in how the callee was
/// found.
pub(crate) extern "C" fn jit_call_native(
    ctx: *mut ExecCtx,
    fn_addr: usize,
    op_id: u64,
    act_id: usize,
    reg_start: usize,
    total: usize,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let f: varn_types::NativeFn = if fn_addr != 0 {
            std::mem::transmute(fn_addr)
        } else {
            match varn_builtins::native_op_fn(op_id) {
                Some(f) => f,
                None => jit_propagate_error(
                    ctx_ref,
                    crate::error::RuntimeError::new(format!("CallNativeOp: unknown op-id {op_id}")),
                ),
            }
        };
        let res = call_native_from_homes(ctx_ref, f, act_id, reg_start, total);
        ctx_ref.jit_native_result = res;
    }
}

/// Gather `[reg_start, reg_start + total)` of activation `act_id` into a
/// `VmValue` slice (per-class home reads via `FrameStore`) and invoke `f`.
unsafe fn call_native_from_homes(
    ctx_ref: &mut ExecCtx,
    f: varn_types::NativeFn,
    act_id: usize,
    reg_start: usize,
    total: usize,
) -> VmValue {
    ctx_ref.record_call_native(f, None);
    let args = ctx_ref.stack.box_range(act_id, reg_start, total);
    if std::env::var_os("VARN_HOME_TRACE").is_some() {
        let fname = ctx_ref
            .frames
            .last()
            .and_then(|fr| fr.closure().proto.name.clone());
        let tags: Vec<String> = args
            .iter()
            .map(|v| {
                let obj = if v.is_heap() {
                    match ctx_ref.heap.get(v.as_heap_idx()) {
                        Some(crate::heap::HeapObj::Str(_)) => ":str",
                        Some(crate::heap::HeapObj::Array(_)) => ":array",
                        Some(crate::heap::HeapObj::Object(_)) => ":object",
                        Some(crate::heap::HeapObj::Instance(_)) => ":instance",
                        Some(crate::heap::HeapObj::VmClosure(_)) => ":closure",
                        Some(_) => ":heap-other",
                        None => ":heap-none",
                    }
                } else {
                    ""
                };
                format!("{:#x}/{:#x}{obj}", v.raw_tag(), v.raw_payload())
            })
            .collect();
        eprintln!(
            "NATIVEOP fn={fname:?} f={:#x} reg_start={reg_start} total={total} args={tags:?}",
            f as usize
        );
    }
    match ctx_ref.invoke_native(f, &args) {
        Ok(v) => v,
        Err(err) => jit_propagate_error(ctx_ref, crate::error::RuntimeError::from(err)),
    }
}
