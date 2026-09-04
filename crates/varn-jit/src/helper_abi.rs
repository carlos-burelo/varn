//! THE list of host entry points compiled code may call.
//!
//! One list, expanded twice: `varn-jit` turns it into the `JitHelpers`
//! struct, `varn-vm` turns it into the table that fills that struct. Before
//! this existed the same ~120 names were written out by hand in both places
//! plus a re-export block, and a name missing from any one of them compiled
//! fine and left a zero in the table — a jump to address 0 from generated
//! code, at runtime, in whichever program first reached that opcode.
//!
//! Each entry is `struct_field => varn_vm_fn`. The two names differ often
//! enough (`bit_and => jit_bitand`, `try_push => jit_push_try`) that the
//! mapping cannot be derived and has to be written down once.
//!
//! Fields that are NOT a plain `fn as usize` — the probed layouts, the
//! struct offsets, the two op-id resolvers — stay hand-written in the
//! struct tail, because they carry types and provenance a name list cannot.

/// Expands `$cb!` with the whole helper list. See the module docs.
#[macro_export]
macro_rules! jit_helper_abi {
    ($cb:ident) => {
        $cb! {
            load_const => jit_load_const,
            eq => jit_eq,
            neq => jit_neq,
            lt => jit_lt,
            lte => jit_lte,
            gt => jit_gt,
            gte => jit_gte,
            add => jit_add,
            sub => jit_sub,
            mul => jit_mul,
            div => jit_div,
            modulo => jit_modulo,
            pow => jit_pow,
            to_string => jit_to_string,
            load_upvalue => jit_load_upvalue,
            store_upvalue => jit_store_upvalue,
            make_closure => jit_make_closure,
            load_static_fn => jit_load_static_fn,
            call => jit_call,
            call_method => jit_call_method,
            call_method_flat => jit_call_method_flat,
            invoke_virtual_flat => jit_invoke_virtual_flat,
            get_property => jit_get_property,
            /// Flat-args variant of `get_property` for the CLIF backend:
            /// `fn(ctx, closure, obj, name_idx, cs_idx, dest, ip) -> VmValue`.
            get_property_flat => jit_get_property_flat,
            set_property => jit_set_property,
            /// Flat-args variant of `set_property` for the CLIF backend:
            /// `fn(ctx, closure, obj, val, name_idx, cs_idx, ip)`.
            set_property_flat => jit_set_property_flat,
            build_array => jit_build_array,
            build_str => jit_build_str,
            negate => jit_negate,
            logical_not => jit_logical_not,
            get_index => jit_get_index,
            set_index => jit_set_index,
            jit_array_get_fast => jit_array_get_fast,
            jit_array_set_fast => jit_array_set_fast,
            typeof_val => jit_typeof_val,
            instanceof => jit_instanceof,
            array_length => jit_array_length,
            array_push => jit_array_push,
            array_pop => jit_array_pop,
            array_extend => jit_array_extend,
            str_concat => jit_str_concat,
            str_slice => jit_str_slice,
            str_length => jit_str_length,
            str_starts_with => jit_str_starts_with,
            str_ends_with => jit_str_ends_with,
            bit_and => jit_bitand,
            bit_or => jit_bitor,
            bit_xor => jit_bitxor,
            shl => jit_shl,
            shr => jit_shr,
            ushr => jit_ushr,
            load_module => jit_load_module,
            load_module_slot => jit_load_module_slot,
            store_module_slot => jit_store_module_slot,
            build_object_with_shape => jit_build_object_with_shape,
            build_record_with_shape => jit_build_record_with_shape,
            range => jit_range,
            assert_not_null => jit_assert_not_null,
            close_upvalue => jit_close_upvalue,
            get_enum_tag => jit_get_enum_tag,
            is_array => jit_is_array_stub,
            wrap_spread => jit_wrap_spread_stub,
            object_keys => jit_object_keys_stub,
            op_in => jit_op_in_stub,
            object_merge => jit_object_merge_stub,
            get_fixed_field => jit_get_fixed_field,
            set_fixed_field => jit_set_fixed_field,
            get_property_maybe => jit_get_property_maybe_stub,
            get_super => jit_get_super,
            get_symbol => jit_get_symbol,
            bind_method => jit_bind_method,
            declare_field => jit_declare_field,
            make_class => jit_make_class,
            inherit => jit_inherit,
            class_member_op => jit_class_member_op,
            build_object => jit_build_object,
            object_rest => jit_object_rest,
            make_enum_variant => jit_make_enum_variant,
            spawn => jit_spawn,
            call_spread => jit_call_spread,
            load_module_by_idx => jit_load_module_by_idx,
            invoke_virtual => jit_invoke_virtual,
            try_push => jit_push_try,
            try_pop => jit_pop_try,
            throw => jit_throw,
            await_helper => jit_await,
            yield_helper => jit_yield,
            get_property_ic_fast => jit_get_property_ic_fast,
            get_property_maybe_ic_fast => jit_get_property_maybe_ic_fast,
            jit_prepare_call => jit_prepare_call,
            jit_push_self_frame => jit_push_self_frame,
            jit_post_call => jit_post_call,
            jit_ensure_stack_capacity => jit_ensure_stack_capacity,
            dispatch_intrinsic => jit_dispatch_intrinsic,
            /// `extern "C" fn(*mut ExecCtx, receiver, pos) -> VmValue` — direct
            /// `charCodeAt` without the stack-window flush/reload overhead.
            str_char_code_at => jit_str_char_code_at,
            /// `extern "C" fn(*mut ExecCtx, receiver: VmValue) -> *const u8` —
            /// address of the receiver's bytes when it is a heap-allocated ASCII
            /// string, `0` otherwise. Hoisted out of a loop by
            /// `preheader::emit_str_caches`; only valid while the region
            /// allocates nothing.
            str_ascii_bytes => jit_str_ascii_bytes,
            /// `extern "C" fn(*mut ExecCtx, receiver: VmValue) -> i64` — byte
            /// length of what `str_ascii_bytes` returned. Meaningless unless
            /// that call answered non-zero for the same receiver.
            str_ascii_len => jit_str_ascii_len,
            /// `extern "C" fn(*mut ExecCtx, receiver, start, end) -> VmValue` — direct
            /// `substring` without the stack-window flush/reload overhead.
            str_substring_intrinsic => jit_str_substring_intrinsic,
            /// `extern "C" fn(*mut ExecCtx, receiver, start, end) -> VmValue` — direct
            /// `slice` without the stack-window flush/reload overhead.
            str_slice_intrinsic => jit_str_slice_intrinsic,
            str_starts_with_intrinsic => jit_str_starts_with_intrinsic,
            str_ends_with_intrinsic => jit_str_ends_with_intrinsic,
            str_includes_intrinsic => jit_str_includes_intrinsic,
            str_index_of_intrinsic => jit_str_index_of_intrinsic,
            str_last_index_of_intrinsic => jit_str_last_index_of_intrinsic,
            jit_is_native_fn => jit_is_native_fn,
            jit_call_native_fast => jit_call_native_fast,
            jit_call_native_op => jit_call_native_op,
            /// `extern "C" fn(*mut ExecCtx, fn_addr, args_start, total)` — direct
            /// native call with the function pointer already resolved.
            jit_call_native_fnptr => jit_call_native_fnptr,
            /// `extern "C" fn(*mut ExecCtx)` — loop back-edge GC safepoint.
            gc_safepoint => jit_gc_safepoint,
            /// `extern "C" fn(*mut ExecCtx, callee: VmValue, argc, a0..a3) -> VmValue`
            /// — the CLIF static-call IC miss path: dispatch the (rebound or
            /// GC-moved) callee through the interpreter/JIT with boxed args.
            clif_call_fallback => clif_call_fallback,
            /// `extern "C" fn(*mut ExecCtx, class_id: u32) -> VmValue`
            /// — Fast allocator for class instances without interpreter frame overhead.
            alloc_instance_fast => jit_alloc_instance_fast,
        }
    };
}
