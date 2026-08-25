//! The `JitHelpers` table: every host entry point compiled code can call,
//! plus the probed struct offsets it addresses fields through.
//!
//! This is an ABI surface, not VM logic — one wrong or missing field is a
//! jump to a null address from generated code, so it lives on its own.
//!
//! The function-address half is NOT written out here: it is expanded from
//! [`varn_jit::jit_helper_abi`], the same list `varn-jit` builds the struct
//! from. Adding a helper is one line there plus the function itself — it is
//! no longer possible to add a field and forget to fill it, because both
//! sides come from the one list.

use crate::exec::ctx;

/// Fills every function-address field from the shared list, then the tail
/// that is not a plain `fn as usize`.
macro_rules! fill_jit_helpers {
    ( $( $(#[$_attr:meta])* $field:ident => $vm_fn:ident ),* $(,)? ) => {{
        let array_layout = crate::heap::Heap::jit_array_layout();
        // `ExecCtx.stack` is a BARE `Vec<VmValue>`, so its data-pointer word is
        // at the bare-`Vec` ptr offset — NOT `elems_ptr_off`, which since the
        // `ArrayRepr` wrapping is measured relative to the `ArrayRepr` (tag +
        // padding + `Vec`) and so includes the wrapper. `slots_ptr_off` is that
        // bare offset (a `Vec`'s field layout is element-type-independent, so
        // the `Vec<Option<HeapObj>>` probe yields the same ptr offset as a
        // `Vec<VmValue>`).
        let stack_data_offset =
            std::mem::offset_of!(ctx::ExecCtx, stack) + array_layout.slots_ptr_off;
        varn_jit::JitHelpers {
            $( $field: ctx::$vm_fn as *const () as usize, )*
            resolve_native_op: resolve_native_op_target,
            array_layout,
            object_layout: crate::heap::Heap::jit_object_layout(),
            str_layout: crate::heap::Heap::jit_str_layout(),
            open_upvalues_offset: {
                let dummy = std::mem::MaybeUninit::<ctx::ExecCtx>::uninit();
                let dummy_ptr = dummy.as_ptr();
                unsafe {
                    (std::ptr::addr_of!((*dummy_ptr).open_upvalues) as usize) - (dummy_ptr as usize)
                }
            },
            pending_constructors_offset: {
                let dummy = std::mem::MaybeUninit::<ctx::ExecCtx>::uninit();
                let dummy_ptr = dummy.as_ptr();
                unsafe {
                    (std::ptr::addr_of!((*dummy_ptr).pending_constructors) as usize)
                        - (dummy_ptr as usize)
                }
            },
            heap_field_offset: std::mem::offset_of!(ctx::ExecCtx, heap),
            nursery_len_offset: crate::heap::Heap::nursery_len_byte_offset_from_rcbox(),
            nursery_threshold: crate::nursery::Nursery::FULL_THRESHOLD,
            jit_native_result_offset: std::mem::offset_of!(ctx::ExecCtx, jit_native_result),
            globals_offset: std::mem::offset_of!(ctx::ExecCtx, globals),
            stack_data_offset,
            frame_prepushed_offset: std::mem::offset_of!(ctx::ExecCtx, jit_frame_prepushed),
            jit_resume_ip_offset: std::mem::offset_of!(ctx::ExecCtx, jit_resume_ip),
            jit_call_dest_offset: std::mem::offset_of!(ctx::ExecCtx, jit_call_dest),
        }
    }};
}

/// Build the production `JitHelpers` table. All fields are static (function
/// addresses + host-struct offsets + probed layouts) — no live `ExecCtx`
/// needed — so both `compile_jit` and the `vn debug -p clif` inspection path
/// share this single source of truth.
pub fn build_jit_helpers() -> varn_jit::JitHelpers {
    varn_jit::jit_helper_abi!(fill_jit_helpers)
}

/// Compile-time op-id resolution for `CallNativeOp` codegen.
///
/// See [`varn_types::NativeOpTarget`] for what each field means and what a zero
/// in it implies. The whole op table lives in `varn-builtins`, which `varn-jit`
/// deliberately does not depend on; this is the function pointer bridging that.
fn resolve_native_op_target(op_id: u64) -> varn_types::NativeOpTarget {
    varn_builtins::find_native_op_entry(op_id).map_or(varn_types::NativeOpTarget::unknown(), |e| {
        varn_types::NativeOpTarget {
            func_ptr: e.func_ptr as usize,
            raw_func_ptr: e.raw_func_ptr as usize,
            signature: e.signature,
        }
    })
}
