use crate::exec::ctx;
use varn_op_macros::jit_helper_table;

macro_rules! fill_tail {
    ( $( $field:ident : $path:path ),* $(,)? ) => {{
        let array_layout = crate::heap::Heap::jit_array_layout();
        varn_jit::JitHelpers {
            $( $field: $path as *const () as usize, )*
        resolve_native_op: resolve_native_op_target,
            array_layout,
            object_layout: crate::heap::Heap::jit_object_layout(),
            instance_alloc: crate::heap::Heap::jit_instance_alloc(),
            heap_field_offset: std::mem::offset_of!(ctx::ExecCtx, heap),
            young_len_offset: crate::heap::Heap::young_len_byte_offset_from_rcbox(),
            young_threshold: crate::heap::young::YOUNG_THRESHOLD,
            jit_native_result_offset: std::mem::offset_of!(ctx::ExecCtx, jit_native_result),
            jit_exit_offset: std::mem::offset_of!(ctx::ExecCtx, jit_exit),
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
            poly_ic_slot_size: varn_types::chunk::POLY_IC_SLOT_SIZE,
            frame_layout: super::frame_layout::probe(),
            call_layout: super::call_layout::probe(),
        }
    }};
}

pub fn build_jit_helpers() -> varn_jit::JitHelpers {
    jit_helper_table!(fill, "src/exec/jit_helpers")
}

fn resolve_native_op_target(op_id: u64) -> varn_types::NativeOpTarget {
    varn_builtins::find_native_op_entry(op_id).map_or(varn_types::NativeOpTarget::unknown(), |e| {
        varn_types::NativeOpTarget {
            func_ptr: e.func_ptr as usize,
            raw_func_ptr: e.raw_func_ptr as usize,
            signature: e.signature,
        }
    })
}
