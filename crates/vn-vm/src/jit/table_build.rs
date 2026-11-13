use crate::exec::ctx;
use vn_macros::jit_helper_table;

macro_rules! fill_tail {
    ( $( $field:ident : $path:path ),* $(,)? ) => {{
        let array_layout = crate::heap::Heap::jit_array_layout();
        vn_jit::JitHelpers {
            $( $field: $path as *const () as usize, )*
        resolve_native_op: resolve_native_op_target,
            array_layout,
            buffer_layout: crate::heap::Heap::jit_buffer_layout(),
            object_layout: crate::heap::Heap::jit_object_layout(),
            instance_alloc: crate::heap::Heap::jit_instance_alloc(),
            heap_field_offset: std::mem::offset_of!(ctx::ExecCtx, heap),
            young_len_offset: crate::heap::Heap::young_len_byte_offset_from_rcbox(),
            young_limit_offset: crate::heap::Heap::young_limit_byte_offset_from_rcbox(),
            jit_native_result_offset: std::mem::offset_of!(ctx::ExecCtx, jit_native_result),
            jit_error_flag_offset: std::mem::offset_of!(ctx::ExecCtx, jit_error_flag),
            jit_site_ip_offset: std::mem::offset_of!(ctx::ExecCtx, frames)
                + crate::frame_stack::FrameStack::pending_field_offset(),
            jit_exit_offset: std::mem::offset_of!(ctx::ExecCtx, jit_exit),
            jit_stack_floor_offset: std::mem::offset_of!(ctx::ExecCtx, jit_stack_floor),
            globals_offset: std::mem::offset_of!(ctx::ExecCtx, globals),
            globals_store_offset: 2 * std::mem::size_of::<usize>()
                + std::mem::offset_of!(crate::globals::GlobalStore, values)
                + array_layout.vec_ptr_off,
            closure_module_base_offset: std::mem::offset_of!(
                crate::closure::VmClosure,
                module_base
            ),
            closure_ic_entries_offset: std::mem::offset_of!(
                crate::closure::VmClosure,
                ic_entries
            ),
            poly_ic_slot_size: vn_types::chunk::POLY_IC_SLOT_SIZE,
            frame_layout: super::frame_layout::probe(),
            call_layout: super::call_layout::probe(),
            call_targets: Box::leak(Box::new([ $( $path as *const () as usize, )* ])),
        }
    }};
}

pub fn build_jit_helpers(settings: &crate::settings::ExecSettings) -> vn_jit::JitHelpers {
    assert_eq!(
        vn_jit::JIT_HELPERS_VERSION,
        16,
        "JitHelpers ABI version mismatch: review offset_of layout after vn-jit-abi change"
    );
    static HELPERS: std::sync::OnceLock<vn_jit::JitHelpers> = std::sync::OnceLock::new();
    let helpers = *HELPERS.get_or_init(|| jit_helper_table!(fill, "src/exec/jit_helpers"));
    let _ = settings;
    helpers
}

fn resolve_native_op_target(op_id: u64) -> vn_types::NativeOpTarget {
    if let Some(runtime) = crate::exec::contract::runtime_op(op_id) {
        return vn_types::NativeOpTarget {
            func_ptr: runtime as usize,
            ..vn_types::NativeOpTarget::unknown()
        };
    }
    vn_builtins::find_native_op_entry(op_id).map_or(vn_types::NativeOpTarget::unknown(), |e| {
        vn_types::NativeOpTarget {
            func_ptr: e.func_ptr as usize,
            raw_func_ptr: e.raw_func_ptr as usize,
            signature: e.signature,
        }
    })
}
