use varn_types::{NativeOpTarget, VmValue};

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct JitArrayLayout {
    pub state_off: usize,

    pub kind_off: usize,

    pub young_state: usize,

    pub vec_ptr_off: usize,

    pub array_tag: usize,

    pub str_tag: usize,

    pub payload_off: usize,

    pub disc_off: usize,

    pub elems_ptr_off: usize,
    pub elems_len_off: usize,
}

#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct JitInstanceAlloc {
    pub cells_off: usize,
    pub classes_off: usize,

    pub young_off: usize,
    pub born_off: usize,

    pub native_off: usize,

    pub class_tag: usize,

    pub class_ref_off: usize,
    pub class_id_off: usize,

    pub hotspot_off: usize,
}

#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct JitObjectLayout {
    pub object_tag: usize,

    pub payload_off: usize,

    pub len_off: usize,

    pub values_off: usize,

    pub shape_off: usize,

    pub shape_id_off: usize,
}

#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct JitCallLayout {
    pub closure_tag: usize,

    pub closure_payload_off: usize,

    pub rc_value_off: usize,

    pub rc_strong_off: usize,

    pub closure_proto_off: usize,

    pub proto_native_off: usize,
    pub proto_native_sig_off: usize,
    pub proto_epoch_off: usize,

    pub frames_ptr_off: usize,
    pub frames_len_off: usize,
    pub frames_cap_off: usize,

    pub frame_size: usize,
    pub frame_closure_ptr_off: usize,
    pub frame_owned_off: usize,
    pub frame_ip_off: usize,
    pub frame_base_off: usize,
    pub frame_class_off: usize,
    pub frame_return_reg_off: usize,

    pub class_vtable_ptr_off: usize,
    pub class_vtable_len_off: usize,
    pub class_vtable_version_off: usize,
    pub no_activation: usize,
    pub no_return_reg: usize,
    pub max_call_depth: usize,
}

#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct JitFrameLayout {
    pub gpr_ptr_offset: usize,
    pub fpr_ptr_offset: usize,
    pub refs_ptr_offset: usize,
    pub dyn_ptr_offset: usize,

    pub allocs_ptr_offset: usize,

    pub alloc_size: usize,

    pub alloc_bases_offset: usize,
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct JitHelpers {
    pub add: usize,
    pub alloc_instance: usize,
    pub array_extend: usize,
    pub array_length: usize,
    pub array_pop: usize,
    pub array_push: usize,
    pub assert_not_null: usize,
    pub await_helper: usize,
    pub bind_method: usize,
    pub bit_and: usize,
    pub bit_or: usize,
    pub bit_xor: usize,
    pub build_array_window: usize,
    pub build_empty_object: usize,
    pub build_map_window: usize,
    pub build_object_window: usize,
    pub build_object_with_shape: usize,
    pub build_record_with_shape: usize,
    pub build_str: usize,
    pub bytes_length: usize,
    pub class_member_op: usize,
    pub close_upvalue: usize,
    pub convert: usize,
    pub declare_layout: usize,
    pub dispatch_intrinsic: usize,
    pub div: usize,
    pub eq: usize,
    pub gc_safepoint: usize,
    pub get_enum_tag: usize,
    pub get_fixed_field: usize,
    pub get_index: usize,
    pub get_property_flat: usize,
    pub get_property_maybe: usize,
    pub get_super: usize,
    pub get_symbol: usize,
    pub gt: usize,
    pub gte: usize,
    pub inherit: usize,
    pub instanceof: usize,
    pub intrinsic_window: usize,
    pub invoke_dynamic: usize,
    pub is_array: usize,
    pub jit_array_get_fast: usize,
    pub jit_array_set_fast: usize,
    pub jit_call_leave: usize,
    pub jit_call_method_cached_window: usize,
    pub jit_call_method_window: usize,
    pub jit_call_native_window: usize,
    pub jit_call_self_window: usize,
    pub jit_call_spread_window: usize,
    pub jit_invoke_window: usize,
    pub jit_push_native_frame: usize,
    pub jit_release_closure: usize,
    pub load_const: usize,
    pub load_global_by_name: usize,
    pub load_module: usize,
    pub load_module_slot: usize,
    pub load_static_fn: usize,
    pub load_upvalue: usize,
    pub logical_not: usize,
    pub lt: usize,
    pub lte: usize,
    pub make_class: usize,
    pub make_closure: usize,
    pub make_closure_window: usize,
    pub make_enum_variant: usize,
    pub make_enum_variant_const: usize,
    pub modulo: usize,
    pub mul: usize,
    pub negate: usize,
    pub neq: usize,
    pub object_keys: usize,
    pub object_merge: usize,
    pub object_rest: usize,
    pub object_rest_window: usize,
    pub op_in: usize,
    pub pow: usize,
    pub range: usize,
    pub set_fixed_field: usize,
    pub set_index: usize,
    pub set_property: usize,
    pub set_property_flat: usize,
    pub shl: usize,
    pub shr: usize,
    pub spawn: usize,
    pub store_global_by_name: usize,
    pub store_module_slot: usize,
    pub store_upvalue: usize,
    pub str_ascii_bytes: usize,
    pub str_ascii_len: usize,
    pub str_char_code_at: usize,
    pub str_concat: usize,
    pub str_ends_with: usize,
    pub str_includes: usize,
    pub str_index_of: usize,
    pub str_length: usize,
    pub str_slice: usize,
    pub str_slice_range: usize,
    pub str_split: usize,
    pub str_starts_with: usize,
    pub sub: usize,
    pub throw: usize,
    pub to_string: usize,
    pub truthy: usize,
    pub try_pop: usize,
    pub try_push: usize,
    pub typeof_val: usize,
    pub ushr: usize,
    pub wrap_spread: usize,
    pub yield_helper: usize,
    pub resolve_native_op: fn(u64) -> NativeOpTarget,
    pub array_layout: JitArrayLayout,
    pub object_layout: JitObjectLayout,
    pub instance_alloc: JitInstanceAlloc,
    pub heap_field_offset: usize,
    pub young_len_offset: usize,
    pub young_threshold: usize,
    pub jit_native_result_offset: usize,
    pub jit_exit_offset: usize,
    pub globals_offset: usize,
    pub globals_store_offset: usize,
    pub closure_module_base_offset: usize,
    pub closure_ic_entries_offset: usize,
    pub poly_ic_slot_size: usize,
    pub frame_layout: JitFrameLayout,
    pub call_layout: JitCallLayout,
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct JitGetIndexArgs {
    pub obj: VmValue,
    pub key: VmValue,
    pub dest: usize,
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct JitSetIndexArgs {
    pub obj: VmValue,
    pub key: VmValue,
    pub val: VmValue,
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct JitSetPropertyArgs {
    pub obj: VmValue,
    pub val: VmValue,
    pub name_idx: usize,
    pub cs_idx: usize,
    pub ip: usize,
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct JitInvokeVirtualArgs {
    pub this_val: VmValue,
    pub name_idx: usize,
    pub arg_start: usize,
    pub arg_count: usize,
    pub dest: usize,
    pub ip: usize,
}
