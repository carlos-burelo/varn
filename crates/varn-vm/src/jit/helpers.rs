//! The `JitHelpers` table: every host entry point compiled code can call,
//! plus the probed struct offsets it addresses fields through.
//!
//! This is an ABI surface, not VM logic — one wrong or missing field is a
//! jump to a null address from generated code, so it lives on its own.
//!
//! The function-address half is generated from the `#[jit_slow(field)]`
//! annotations in `exec::jit_helpers`, the same source `varn-jit` builds the
//! struct fields from. A helper is registered once, at its function.

use crate::exec::ctx;
use varn_op_macros::jit_helper_table;

macro_rules! fill_tail {
    ( $( $field:ident : $path:path ),* $(,)? ) => {{
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
            $( $field: $path as *const () as usize, )*
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
            globals_offset: std::mem::offset_of!(ctx::ExecCtx, globals) + array_layout.slots_ptr_off,
            closure_module_base_offset: std::mem::offset_of!(
                crate::closure::VmClosure,
                module_base
            ),
            closure_ic_entries_offset: std::mem::offset_of!(
                crate::closure::VmClosure,
                ic_entries
            ),
            poly_ic_slot_size: varn_types::chunk::POLY_IC_SLOT_SIZE,
            stack_data_offset,
            frame_layout: super::frame_layout::probe(),
            max_call_depth: crate::exec::jit_helpers::calls::MAX_CALL_DEPTH,
        }
    }};
}

pub fn build_jit_helpers() -> varn_jit::JitHelpers {
    jit_helper_table!(fill, "src/exec/jit_helpers")
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
