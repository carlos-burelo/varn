use super::construct::jit_propagate_error;
use super::suspend::jit_suspend_at;
use crate::exec::ctx::ExecCtx;
use crate::value::VmValue;

#[varn_op_macros::jit_slow(field = "load_module")]
pub(crate) extern "C" fn jit_load_module(
    ctx: *mut ExecCtx,
    closure: *const crate::closure::VmClosure,
    const_idx: usize,
    own_ip: usize,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let closure_ref = &*closure;
        let spec_nv = closure_ref.constants[const_idx];
        let spec = match ctx_ref.heap.str_val(spec_nv) {
            Some(s) => s,
            None => {
                ctx_ref.jit_native_result = VmValue::null();
                return;
            }
        };

        let self_idx = ctx_ref.frames.len() - 1;

        let loaded =
            match ctx_ref.load_module_from_source(&spec, &closure_ref.proto.chunk.source_file) {
                Ok(v) => v,
                Err(e) => jit_propagate_error(ctx_ref, e),
            };
        if ctx_ref.vm_suspend.is_some() {
            jit_suspend_at(ctx_ref, self_idx, own_ip);
        }
        ctx_ref.jit_native_result = loaded;
    }
}

#[varn_op_macros::jit_slow(field = "load_module_slot")]
pub(crate) extern "C" fn jit_load_module_slot(
    ctx: *mut ExecCtx,
    mod_tag: u64,
    mod_payload: u64,
    slot_idx: usize,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let module_val = VmValue::from_raw_parts(mod_tag, mod_payload);
        if !module_val.is_heap() {
            ctx_ref.jit_native_result = VmValue::null();
            return;
        }
        if let Some(crate::heap::HeapObj::Module(m)) = ctx_ref.heap.get(module_val.as_heap()) {
            ctx_ref.jit_native_result = m.get_slot(slot_idx).unwrap_or(VmValue::null());
        } else {
            ctx_ref.jit_native_result = VmValue::null();
        }
    }
}

#[varn_op_macros::jit_slow(field = "store_module_slot")]
pub(crate) extern "C" fn jit_store_module_slot(
    ctx: *mut ExecCtx,
    slot_idx: usize,
    val_tag: u64,
    val_payload: u64,
) {
    unsafe {
        let ctx_ref = &mut *ctx;
        let val_nv = VmValue::from_raw_parts(val_tag, val_payload);
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

        if let Some(crate::heap::HeapObj::Module(m)) = ctx_ref.heap.get_mut(exports_nv.as_heap()) {
            let m = std::rc::Rc::make_mut(m);
            m.set_slot(slot_idx, val_nv);
        } else {
            panic!("OpStoreModuleSlot: active module object is not a ModuleObj");
        }
    }
}
