use crate::value::VmValue;
use std::cell::RefCell;
use std::rc::Rc;
use varn_base::VmValuePayload;
use varn_types::chunk::PolyICSlot;
pub use varn_types::VmValueRef;
use varn_types::{FunctionProto, Value};

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
            Some(slot) => stack[slot],
            None => g.value,
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
    pub fn new(proto: Rc<FunctionProto>, constants: Vec<VmValue>) -> Self {
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
        closure.compile_jit();
        closure
    }

    pub fn with_upvalues(
        proto: Rc<FunctionProto>,
        upvalues: Vec<VmUpvalue>,
        constants: Rc<Vec<VmValue>>,
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
        closure.compile_jit();
        closure
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

        let helpers = varn_jit::JitHelpers {
            load_const: crate::exec::ctx::jit_load_const as usize,
            load_global_idx: crate::exec::ctx::jit_load_global_idx as usize,
            store_global_idx: crate::exec::ctx::jit_store_global_idx as usize,
            define_global_idx: crate::exec::ctx::jit_define_global_idx as usize,
            eq: crate::exec::ctx::jit_eq as usize,
            neq: crate::exec::ctx::jit_neq as usize,
            lt: crate::exec::ctx::jit_lt as usize,
            lte: crate::exec::ctx::jit_lte as usize,
            gt: crate::exec::ctx::jit_gt as usize,
            gte: crate::exec::ctx::jit_gte as usize,
            add: crate::exec::ctx::jit_add as usize,
            sub: crate::exec::ctx::jit_sub as usize,
            mul: crate::exec::ctx::jit_mul as usize,
            div: crate::exec::ctx::jit_div as usize,
            modulo: crate::exec::ctx::jit_modulo as usize,
            pow: crate::exec::ctx::jit_pow as usize,
            to_string: crate::exec::ctx::jit_to_string as usize,
            load_global: crate::exec::ctx::jit_load_global as usize,
            load_upvalue: crate::exec::ctx::jit_load_upvalue as usize,
            store_upvalue: crate::exec::ctx::jit_store_upvalue as usize,
            make_closure: crate::exec::ctx::jit_make_closure as usize,
            call: crate::exec::ctx::jit_call as usize,
            call_method: crate::exec::ctx::jit_call_method as usize,
            get_property: crate::exec::ctx::jit_get_property as usize,
            set_property: crate::exec::ctx::jit_set_property as usize,
            build_array: crate::exec::ctx::jit_build_array as usize,
            build_str: crate::exec::ctx::jit_build_str as usize,
            negate: crate::exec::ctx::jit_negate as usize,
            logical_not: crate::exec::ctx::jit_logical_not as usize,
            get_index: crate::exec::ctx::jit_get_index as usize,
            set_index: crate::exec::ctx::jit_set_index as usize,
            typeof_val: crate::exec::ctx::jit_typeof_val as usize,
            instanceof: crate::exec::ctx::jit_instanceof as usize,
            array_length: crate::exec::ctx::jit_array_length as usize,
            array_push: crate::exec::ctx::jit_array_push as usize,
            array_pop: crate::exec::ctx::jit_array_pop as usize,
            array_extend: crate::exec::ctx::jit_array_extend as usize,
            str_concat: crate::exec::ctx::jit_str_concat as usize,
            str_slice: crate::exec::ctx::jit_str_slice as usize,
            str_length: crate::exec::ctx::jit_str_length as usize,
            bit_and: crate::exec::ctx::jit_bitand as usize,
            bit_or: crate::exec::ctx::jit_bitor as usize,
            bit_xor: crate::exec::ctx::jit_bitxor as usize,
            shl: crate::exec::ctx::jit_shl as usize,
            shr: crate::exec::ctx::jit_shr as usize,
            ushr: crate::exec::ctx::jit_ushr as usize,
            load_module: crate::exec::ctx::jit_load_module as usize,
            load_module_slot: crate::exec::ctx::jit_load_module_slot as usize,
            build_object_with_shape: crate::exec::ctx::jit_build_object_with_shape as usize,
            range: crate::exec::ctx::jit_range as usize,
            assert_not_null: crate::exec::ctx::jit_assert_not_null as usize,
            close_upvalue: crate::exec::ctx::jit_close_upvalue as usize,
            get_enum_tag: crate::exec::ctx::jit_get_enum_tag as usize,
            is_array: crate::exec::ctx::jit_is_array_stub as usize,
            wrap_spread: crate::exec::ctx::jit_wrap_spread_stub as usize,
            object_keys: crate::exec::ctx::jit_object_keys_stub as usize,
            op_in: crate::exec::ctx::jit_op_in_stub as usize,
            object_merge: crate::exec::ctx::jit_object_merge_stub as usize,
            get_fixed_field: crate::exec::ctx::jit_get_fixed_field as usize,
            set_fixed_field: crate::exec::ctx::jit_set_fixed_field as usize,
            get_property_maybe: crate::exec::ctx::jit_get_property_maybe_stub as usize,
            get_super: crate::exec::ctx::jit_get_super as usize,
            get_symbol: crate::exec::ctx::jit_get_symbol as usize,
            bind_method: crate::exec::ctx::jit_bind_method as usize,
            define_global: crate::exec::ctx::jit_define_global as usize,
            store_global: crate::exec::ctx::jit_store_global as usize,
            declare_field: crate::exec::ctx::jit_declare_field as usize,
            make_class: crate::exec::ctx::jit_make_class as usize,
            inherit: crate::exec::ctx::jit_inherit as usize,
            class_member_op: crate::exec::ctx::jit_class_member_op as usize,
            build_object: crate::exec::ctx::jit_build_object as usize,
            object_rest: crate::exec::ctx::jit_object_rest as usize,
            make_enum_variant: crate::exec::ctx::jit_make_enum_variant as usize,
            call_spread: crate::exec::ctx::jit_call_spread as usize,
            load_module_by_idx: crate::exec::ctx::jit_load_module_by_idx as usize,
        };
        match varn_jit::compile(&self.proto, helpers) {
            Ok((entry, code)) => {
                self.jit_entry = Some(entry);
                self.jit_code = Some(code.clone());

                let entry_usize: usize = unsafe { std::mem::transmute(entry) };
                self.proto.jit_entry.set(Some(entry_usize));
                *self.proto.jit_code.borrow_mut() = Some(code);
            }
            Err(e) => {
                self.proto.jit_failed.set(true);
                if std::env::var("VARN_JIT_DEBUG").is_ok() {
                    eprintln!(
                        "JIT compilation failed for '{}': {}",
                        self.proto.name.as_deref().unwrap_or("<anonymous>"),
                        e
                    );
                }
            }
        }
    }

    fn make_ic_slots(count: usize) -> Vec<PolyICSlot> {
        let mut v = Vec::with_capacity(count);
        for _ in 0..count {
            v.push(PolyICSlot::new());
        }
        v
    }

    #[inline(always)]
    pub fn ic_cache_len(&self) -> usize {
        self.ic_cache.borrow().len()
    }
}

pub struct CallFrame {
    pub closure: Rc<VmClosure>,
    pub ip: usize,
    pub base: usize,
    pub current_class: Option<Rc<varn_types::ClassObj>>,
    pub return_reg: Option<u16>,
}

impl CallFrame {
    pub fn new(closure: Rc<VmClosure>, base: usize) -> Self {
        Self {
            closure,
            ip: 0,
            base,
            current_class: None,
            return_reg: None,
        }
    }

    #[inline(always)]
    pub fn proto(&self) -> &FunctionProto {
        &self.closure.proto
    }

    #[inline(always)]
    pub fn code(&self) -> &[u16] {
        &self.closure.proto.chunk.code
    }

    #[inline(always)]
    pub fn read_u16(&self) -> u16 {
        self.code()[self.ip]
    }
}

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
