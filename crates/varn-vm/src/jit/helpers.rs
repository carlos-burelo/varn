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
            $( $field: ctx::$vm_fn as usize, )*
            resolve_native_op: resolve_native_op_addr,
            resolve_native_op_v2: resolve_native_op_addr_v2,
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
            gc_safepoint: ctx::jit_gc_safepoint as usize,
            heap_field_offset: std::mem::offset_of!(ctx::ExecCtx, heap),
            nursery_len_offset: crate::heap::Heap::nursery_len_byte_offset_from_rcbox(),
            nursery_threshold: crate::nursery::Nursery::FULL_THRESHOLD,
            jit_native_result_offset: std::mem::offset_of!(ctx::ExecCtx, jit_native_result),
            globals_offset: std::mem::offset_of!(ctx::ExecCtx, globals),
            stack_data_offset,
            frame_prepushed_offset: std::mem::offset_of!(ctx::ExecCtx, jit_frame_prepushed),
            jit_resume_ip_offset: std::mem::offset_of!(ctx::ExecCtx, jit_resume_ip),
            jit_call_dest_offset: std::mem::offset_of!(ctx::ExecCtx, jit_call_dest),
            clif_call_fallback: ctx::clif_call_fallback as usize,
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

/// Compile-time op-id resolution for `CallNativeOp` codegen: returns the
/// native fn address, or 0 when the op-id is unknown (codegen then falls
/// back to the runtime-resolving helper, which raises the proper error).
fn resolve_native_op_addr(op_id: u64) -> usize {
    varn_builtins::native_op_fn(op_id).map_or(0, |f| f as usize)
}

fn resolve_native_op_addr_v2(op_id: u64) -> (usize, usize, varn_types::SignatureDescriptor) {
    varn_builtins::find_native_op_entry(op_id).map_or(
        (0, 0, varn_types::SignatureDescriptor::empty()),
        |e| (e.func_ptr as usize, e.raw_func_ptr as usize, e.signature),
    )
}

#[cfg(test)]
mod build_jit_helpers_tests {
    /// Asserts EVERY entry in the shared list resolved to a real address.
    ///
    /// This is the check the hand-written table could not have: a field left
    /// at 0 used to be a jump to address 0 from generated code, discovered
    /// only when some program first reached that opcode. Expanding the same
    /// list the table is built from means a helper cannot be added without
    /// this test covering it.
    macro_rules! assert_every_helper_address {
        ( $( $(#[$_attr:meta])* $field:ident => $_vm_fn:ident ),* $(,)? ) => {{
            let h = super::build_jit_helpers();
            $(
                assert_ne!(
                    h.$field, 0,
                    concat!("jit helper `", stringify!($field), "` is a null address")
                );
            )*
        }};
    }

    #[test]
    fn every_helper_address_is_real() {
        varn_jit::jit_helper_abi!(assert_every_helper_address);
    }

    #[test]
    fn build_jit_helpers_has_real_addresses() {
        let h = super::build_jit_helpers();
        // The two address fields outside the shared list (they sit in the
        // hand-written tail), plus the probed nursery threshold — proving this
        // is the production construction and not a zeroed stub.
        assert_ne!(h.gc_safepoint, 0);
        assert_ne!(h.clif_call_fallback, 0);
        assert!(h.nursery_threshold > 0);
    }
}
