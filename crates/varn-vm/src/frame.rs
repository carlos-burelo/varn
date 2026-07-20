use crate::exec::ctx;
use crate::value::VmValue;
use std::cell::RefCell;
use std::rc::Rc;
use varn_base::VmValuePayload;
use varn_types::chunk::PolyICSlot;
use varn_types::FunctionProto;
pub use varn_types::VmValueRef;

#[derive(Debug, Clone)]
pub struct VmUpvalue {
    pub inner: Rc<RefCell<VmUpvalueInner>>,
}

#[derive(Debug, Clone)]
pub struct VmUpvalueInner {
    pub value: VmValue,
    pub stack_slot: Option<usize>,
}

impl VmUpvalue {
    pub fn open(stack_slot: usize) -> Self {
        Self {
            inner: Rc::new(RefCell::new(VmUpvalueInner {
                value: VmValue::null(),
                stack_slot: Some(stack_slot),
            })),
        }
    }

    pub fn closed(value: VmValue) -> Self {
        Self {
            inner: Rc::new(RefCell::new(VmUpvalueInner {
                value,
                stack_slot: None,
            })),
        }
    }

    pub fn read(&self, stack: &[VmValue]) -> VmValue {
        let g = self.inner.borrow_mut();
        match g.stack_slot {
            Some(slot) => {
                let v = stack[slot];
                v
            }
            None => {
                let v = g.value;
                v
            }
        }
    }

    pub fn write(&self, val: VmValue, stack: &mut Vec<VmValue>) {
        let mut g = self.inner.borrow_mut();
        match g.stack_slot {
            Some(slot) => stack[slot] = val,
            None => g.value = val,
        }
    }

    pub fn close(&self, stack: &[VmValue]) {
        let mut g = self.inner.borrow_mut();
        if let Some(slot) = g.stack_slot.take() {
            g.value = stack[slot];
        }
    }
}

#[derive(Debug, Clone)]
#[repr(C)]
pub struct VmClosure {
    pub proto: Rc<FunctionProto>,
    pub upvalues: Vec<VmUpvalue>,
    pub constants: Rc<Vec<VmValue>>,
    pub ic_cache: Rc<RefCell<Vec<PolyICSlot>>>,
    pub feedback: Rc<RefCell<varn_types::chunk::FeedbackVector>>,
    pub jit_entry: Option<varn_jit::JitFn>,
    pub jit_code: Option<Rc<dyn std::any::Any>>,
}

impl VmClosure {
    pub fn new(
        proto: Rc<FunctionProto>,
        constants: Vec<VmValue>,
        settings: crate::settings::ExecSettings,
    ) -> Self {
        proto.ensure_ic();
        let ic_cache = Rc::clone(&proto.ic_cache);
        let feedback = Rc::clone(&proto.feedback);
        let mut closure = Self {
            proto,
            upvalues: Vec::new(),
            constants: Rc::new(constants),
            ic_cache,
            feedback,
            jit_entry: None,
            jit_code: None,
        };
        // `no_jit` means the JIT does not run at all, not merely that its
        // output goes unused: a run meant to isolate a codegen bug should not
        // be invoking codegen.
        if !settings.no_jit {
            closure.compile_jit();
        }
        closure
    }

    pub fn with_upvalues(
        proto: Rc<FunctionProto>,
        upvalues: Vec<VmUpvalue>,
        constants: Rc<Vec<VmValue>>,
        settings: crate::settings::ExecSettings,
    ) -> Self {
        proto.ensure_ic();
        let ic_cache = Rc::clone(&proto.ic_cache);
        let feedback = Rc::clone(&proto.feedback);
        let mut closure = Self {
            proto,
            upvalues,
            constants,
            ic_cache,
            feedback,
            jit_entry: None,
            jit_code: None,
        };
        // `no_jit` means the JIT does not run at all, not merely that its
        // output goes unused: a run meant to isolate a codegen bug should not
        // be invoking codegen.
        if !settings.no_jit {
            closure.compile_jit();
        }
        closure
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
            |e| (e.func_ptr as usize, e.raw_func_ptr as usize, e.signature)
        )
    }

    pub fn compile_jit(&mut self) {
        if self.proto.jit_failed.get() {
            return;
        }
        if let Some(entry_usize) = self.proto.jit_entry.get() {
            let entry: varn_jit::JitFn = unsafe { std::mem::transmute(entry_usize) };
            self.jit_entry = Some(entry);
            self.jit_code = self.proto.jit_code.borrow().clone();
            varn_jit::JIT_STATS
                .jit_cached
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            return;
        }

        let array_layout = crate::heap::Heap::jit_array_layout();
        let stack_data_offset =
            std::mem::offset_of!(ctx::ExecCtx, stack) + array_layout.elems_ptr_off;
        let helpers = varn_jit::JitHelpers {
            load_const: ctx::jit_load_const as usize,
            load_global_idx: ctx::jit_load_global_idx as usize,
            store_global_idx: ctx::jit_store_global_idx as usize,
            define_global_idx: ctx::jit_define_global_idx as usize,
            eq: ctx::jit_eq as usize,
            neq: ctx::jit_neq as usize,
            lt: ctx::jit_lt as usize,
            lte: ctx::jit_lte as usize,
            gt: ctx::jit_gt as usize,
            gte: ctx::jit_gte as usize,
            add: ctx::jit_add as usize,
            sub: ctx::jit_sub as usize,
            mul: ctx::jit_mul as usize,
            div: ctx::jit_div as usize,
            modulo: ctx::jit_modulo as usize,
            pow: ctx::jit_pow as usize,
            to_string: ctx::jit_to_string as usize,
            load_global: ctx::jit_load_global as usize,
            load_upvalue: ctx::jit_load_upvalue as usize,
            store_upvalue: ctx::jit_store_upvalue as usize,
            make_closure: ctx::jit_make_closure as usize,
            load_static_fn: ctx::jit_load_static_fn as usize,
            call: ctx::jit_call as usize,
            call_method: ctx::jit_call_method as usize,
            get_property: ctx::jit_get_property as usize,
            set_property: ctx::jit_set_property as usize,
            build_array: ctx::jit_build_array as usize,
            build_str: ctx::jit_build_str as usize,
            negate: ctx::jit_negate as usize,
            logical_not: ctx::jit_logical_not as usize,
            get_index: ctx::jit_get_index as usize,
            set_index: ctx::jit_set_index as usize,
            jit_array_get_fast: ctx::jit_array_get_fast as usize,
            jit_array_set_fast: ctx::jit_array_set_fast as usize,
            typeof_val: ctx::jit_typeof_val as usize,
            instanceof: ctx::jit_instanceof as usize,
            array_length: ctx::jit_array_length as usize,
            array_push: ctx::jit_array_push as usize,
            array_pop: ctx::jit_array_pop as usize,
            array_extend: ctx::jit_array_extend as usize,
            str_concat: ctx::jit_str_concat as usize,
            str_slice: ctx::jit_str_slice as usize,
            str_length: ctx::jit_str_length as usize,
            bit_and: ctx::jit_bitand as usize,
            bit_or: ctx::jit_bitor as usize,
            bit_xor: ctx::jit_bitxor as usize,
            shl: ctx::jit_shl as usize,
            shr: ctx::jit_shr as usize,
            ushr: ctx::jit_ushr as usize,
            load_module: ctx::jit_load_module as usize,
            load_module_slot: ctx::jit_load_module_slot as usize,
            store_module_slot: ctx::jit_store_module_slot as usize,
            build_object_with_shape: ctx::jit_build_object_with_shape as usize,
            range: ctx::jit_range as usize,
            assert_not_null: ctx::jit_assert_not_null as usize,
            close_upvalue: ctx::jit_close_upvalue as usize,
            get_enum_tag: ctx::jit_get_enum_tag as usize,
            is_array: ctx::jit_is_array_stub as usize,
            wrap_spread: ctx::jit_wrap_spread_stub as usize,
            object_keys: ctx::jit_object_keys_stub as usize,
            op_in: ctx::jit_op_in_stub as usize,
            object_merge: ctx::jit_object_merge_stub as usize,
            get_fixed_field: ctx::jit_get_fixed_field as usize,
            set_fixed_field: ctx::jit_set_fixed_field as usize,
            get_property_maybe: ctx::jit_get_property_maybe_stub as usize,
            get_super: ctx::jit_get_super as usize,
            get_symbol: ctx::jit_get_symbol as usize,
            bind_method: ctx::jit_bind_method as usize,
            define_global: ctx::jit_define_global as usize,
            store_global: ctx::jit_store_global as usize,
            declare_field: ctx::jit_declare_field as usize,
            make_class: ctx::jit_make_class as usize,
            inherit: ctx::jit_inherit as usize,
            class_member_op: ctx::jit_class_member_op as usize,
            build_object: ctx::jit_build_object as usize,
            object_rest: ctx::jit_object_rest as usize,
            make_enum_variant: ctx::jit_make_enum_variant as usize,
            spawn: ctx::jit_spawn as usize,
            call_spread: ctx::jit_call_spread as usize,
            load_module_by_idx: ctx::jit_load_module_by_idx as usize,
            invoke_virtual: ctx::jit_invoke_virtual as usize,
            try_push: ctx::jit_push_try as usize,
            try_pop: ctx::jit_pop_try as usize,
            throw: ctx::jit_throw as usize,
            await_helper: ctx::jit_await as usize,
            yield_helper: ctx::jit_yield as usize,
            get_property_ic_fast: ctx::jit_get_property_ic_fast as usize,
            get_property_maybe_ic_fast: ctx::jit_get_property_maybe_ic_fast as usize,
            jit_prepare_call: ctx::jit_prepare_call as usize,
            jit_push_self_frame: ctx::jit_push_self_frame as usize,
            jit_post_call: ctx::jit_post_call as usize,
            jit_ensure_stack_capacity: ctx::jit_ensure_stack_capacity as usize,
            dispatch_intrinsic: ctx::jit_dispatch_intrinsic as usize,
            jit_is_native_fn: ctx::jit_is_native_fn as usize,
            jit_call_native_fast: ctx::jit_call_native_fast as usize,
            jit_call_native_op: ctx::jit_call_native_op as usize,
            jit_call_native_fnptr: ctx::jit_call_native_fnptr as usize,
            resolve_native_op: Self::resolve_native_op_addr,
            resolve_native_op_v2: Self::resolve_native_op_addr_v2,
            array_layout,
            object_layout: crate::heap::Heap::jit_object_layout(),
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
            clif_call_fallback: ctx::clif_call_fallback as usize,
        };
        let linker = crate::clif_link::CtxLinker::current();
        match varn_jit::compile(&self.proto, &self.constants, helpers, &linker) {
            Ok((entry, code)) => {
                self.jit_entry = Some(entry);
                self.jit_code = Some(code.clone());

                let entry_usize: usize = unsafe { std::mem::transmute(entry) };
                self.proto.jit_entry.set(Some(entry_usize));
                *self.proto.jit_code.borrow_mut() = Some(code);
            }
            Err(_) => {
                self.proto.jit_failed.set(true);
            }
        }
    }

    #[inline(always)]
    pub fn ic_cache_len(&self) -> usize {
        self.ic_cache.borrow().len()
    }
}

#[repr(C)]
pub struct CallFrame {
    pub closure_ptr: *const VmClosure,
    pub _owned_closure: Option<Rc<VmClosure>>,
    pub ip: usize,
    pub base: usize,
    pub current_class: Option<Rc<varn_types::ClassObj>>,
    pub return_reg: Option<u16>,
}

unsafe impl Send for CallFrame {}
unsafe impl Sync for CallFrame {}

impl CallFrame {
    pub fn new(closure: &VmClosure, base: usize) -> Self {
        Self {
            closure_ptr: closure as *const VmClosure,
            _owned_closure: None,
            ip: 0,
            base,
            current_class: None,
            return_reg: None,
        }
    }

    pub fn new_owned(closure: Rc<VmClosure>, base: usize) -> Self {
        Self {
            closure_ptr: Rc::as_ptr(&closure),
            _owned_closure: Some(closure),
            ip: 0,
            base,
            current_class: None,
            return_reg: None,
        }
    }

    #[inline(always)]
    pub fn closure(&self) -> &VmClosure {
        unsafe { &*self.closure_ptr }
    }

    #[inline(always)]
    pub fn proto(&self) -> &FunctionProto {
        &self.closure().proto
    }

    #[inline(always)]
    pub fn code(&self) -> &[u16] {
        &self.closure().proto.chunk.code
    }

    #[inline(always)]
    pub fn read_u16(&self) -> u16 {
        self.code()[self.ip]
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TryHandler {
    pub catch_ip: usize,
    pub frame_depth: usize,
    pub err_reg: u8,
}

#[derive(Debug, Clone)]
pub struct VmClosurePayload(pub Rc<VmClosure>);

impl VmValuePayload for VmClosurePayload {
    fn clone_payload(&self) -> Box<dyn VmValuePayload> {
        Box::new(self.clone())
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
