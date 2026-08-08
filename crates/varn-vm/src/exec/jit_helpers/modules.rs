//! Module loading and module-slot access from compiled code.
//!
//! `jit_load_module` has to be able to SUSPEND: an imported module may hit a
//! top-level await, and the frame that asked for it must be parked and
//! resumed later, with its ip rewound to re-run the load.

use super::construct::jit_propagate_error;
use super::suspend::jit_suspend_at;
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;

/// `own_ip` is this `LoadModule` instruction's own offset, not the next one:
/// an imported module that suspends on a top-level `await` leaves the import
/// unfinished, so the frame rewinds to re-execute the load once the awaited
/// task resolves — the same rewind `op_load_module` performs interpreted.
pub(crate) extern "C" fn jit_load_module(
    ctx: *mut ExecCtx,
    closure: *const crate::closure::VmClosure,
    const_idx: usize,
    own_ip: usize,
) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let closure_ref = &*closure;
        let spec_nv = closure_ref.constants[const_idx];
        let spec = match ctx_ref.heap.str_val(spec_nv) {
            Some(s) => s,
            None => return VmValue::null(),
        };
        // Our own frame, taken BEFORE the load: a module that suspends leaves
        // its frame on top of ours, so `frames.last()` is no longer us.
        let self_idx = ctx_ref.frames.len() - 1;
        // A failed import is the user's problem, not ours: a missing file or a
        // module that throws while evaluating must surface as a catchable Varn
        // error, exactly as it does interpreted (`op_load_module` propagates
        // with `?`). Panicking here killed the host process instead, so the
        // same `import` was catchable or fatal depending on whether the
        // importing function happened to be compiled.
        let loaded =
            match ctx_ref.load_module_from_source(&spec, &closure_ref.proto.chunk.source_file) {
                Ok(v) => v,
                Err(e) => jit_propagate_error(ctx_ref, e),
            };
        if ctx_ref.vm_suspend.is_some() {
            jit_suspend_at(ctx_ref, self_idx, own_ip);
        }
        loaded
    }
}

pub(crate) extern "C" fn jit_load_module_slot(
    ctx: *mut ExecCtx,
    module_val: VmValue,
    slot_idx: usize,
) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        if !module_val.is_heap() {
            return VmValue::null();
        }
        if let Some(crate::heap::HeapObj::Module(m)) = ctx_ref.heap.get(module_val.as_heap_idx()) {
            m.get_slot(slot_idx).unwrap_or(VmValue::null())
        } else {
            VmValue::null()
        }
    }
}

pub(crate) extern "C" fn jit_store_module_slot(
    ctx: *mut ExecCtx,
    slot_idx: usize,
    val_nv: VmValue,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let caller_depth = ctx_ref.frames.len();
        let frame_idx = caller_depth - 1;

        let exports_nv = if let Some(nv) = ctx_ref.module_exports.get(&frame_idx).copied() {
            nv
        } else {
            panic!("OpStoreModuleSlot: no active module object for current frame");
        };

        if !exports_nv.is_heap() {
            panic!("OpStoreModuleSlot: active module object is not a heap object");
        }

        if let Some(crate::heap::HeapObj::Module(m)) =
            ctx_ref.heap.get_mut(exports_nv.as_heap_idx())
        {
            let m = std::rc::Rc::make_mut(m);
            m.set_slot(slot_idx, val_nv);
        } else {
            panic!("OpStoreModuleSlot: active module object is not a ModuleObj");
        }
    }
}

pub(crate) extern "C" fn jit_load_module_by_idx(
    ctx: *mut ExecCtx,
    closure: *const crate::closure::VmClosure,
    spec_idx: usize,
) -> VmValue {
    unsafe {
        let ctx_ref = &mut *ctx;
        let closure_ref = &*closure;
        let spec_nv = closure_ref.constants[spec_idx];
        let spec = match ctx_ref.heap.str_val(spec_nv) {
            Some(s) => s,
            None => return VmValue::null(),
        };
        // Same reasoning as `jit_load_module`: a failed import is a catchable
        // Varn error, not a host abort. The `eprintln!` that used to accompany
        // the panic went with it — the error now carries the specifier and
        // reaches the user through the normal reporting path.
        match ctx_ref.load_module_from_source(&spec, &closure_ref.proto.chunk.source_file) {
            Ok(v) => v,
            Err(e) => jit_propagate_error(ctx_ref, e),
        }
    }
}
