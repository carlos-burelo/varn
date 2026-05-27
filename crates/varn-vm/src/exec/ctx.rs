use crate::error::{FrameInfo, RuntimeError, VmResult};
use crate::frame::{CallFrame, TryHandler, VmClosure, VmUpvalue};
use crate::globals::GlobalStore;
use crate::heap::{Heap, HeapObj};
use crate::loader::ModuleLoader;
use crate::profile::ProfileCounters;
use crate::value::VmValue;
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use varn_core::ModuleId;
use varn_core::OpCode;
use varn_types::value::LazyTask;
use varn_types::{FunctionProto, Literal, ModuleObj, NativeCtx, PoolEntry, Value};

use crate::linker::Linker;

use super::calls::{self, PreparedCall};
use super::VmSuspend;
use varn_types::generator::GenChannel;

pub use super::ctx_jit_runtime::{
    jit_array_extend, jit_array_length, jit_array_pop, jit_array_push, jit_bitand, jit_bitor,
    jit_bitxor, jit_build_object_with_shape, jit_div, jit_get_index, jit_instanceof,
    jit_load_module, jit_load_module_slot, jit_logical_not, jit_modulo, jit_negate, jit_pow,
    jit_range, jit_set_index, jit_shl, jit_shr, jit_str_concat, jit_str_length, jit_str_slice,
    jit_typeof_val, jit_ushr,
};
pub use super::ctx_jit_values::{
    jit_add, jit_build_array, jit_build_str, jit_call, jit_call_method, jit_define_global_idx,
    jit_eq, jit_get_property, jit_gt, jit_gte, jit_load_const, jit_load_global,
    jit_load_global_idx, jit_load_upvalue, jit_lt, jit_lte, jit_make_closure, jit_mul, jit_neq,
    jit_set_property, jit_store_global_idx, jit_store_upvalue, jit_sub, jit_to_string,
};

#[repr(C)]
pub struct ExecCtx {
    pub stack: Vec<VmValue>,
    pub frames: Vec<CallFrame>,
    pub globals: GlobalStore,
    pub heap: Heap,
    pub try_handlers: Vec<TryHandler>,
    pub modules: FxHashMap<ModuleId, VmValue>,
    pub precompiled: Rc<FxHashMap<ModuleId, Rc<FunctionProto>>>,
    pub loader: Option<Rc<dyn ModuleLoader>>,
    pub trace: bool,
    pub open_upvalues: Vec<(usize, VmUpvalue)>,
    pub pending_constructors: Vec<(usize, VmValue)>,
    pub pending_setters: Vec<(usize, VmValue)>,
    pub vm_suspend: Option<VmSuspend>,
    pub gen_channel: Option<Rc<GenChannel>>,
    pub deferred_tasks: FxHashMap<usize, Rc<LazyTask>>,
    pub module_exports: FxHashMap<usize, VmValue>,
    pub opcode_counts: Option<Rc<Vec<std::sync::atomic::AtomicU64>>>,
    pub profile_counters: Option<Arc<ProfileCounters>>,
    pub proto_constants: FxHashMap<usize, Rc<Vec<VmValue>>>,
    pub no_jit: bool,
    pub linker: Linker,
}

impl ExecCtx {
    pub fn new(mut globals: GlobalStore) -> Self {
        varn_runtime::init_heap();
        let mut heap = Heap::new();

        let fresh = globals.values.is_empty();
        if fresh {
            globals = GlobalStore::with_native_layout(&mut heap);
        }

        let mut ctx = Self {
            stack: Vec::with_capacity(16384),
            frames: Vec::with_capacity(512),
            globals,
            heap,
            try_handlers: Vec::new(),
            modules: FxHashMap::default(),
            precompiled: Rc::new(FxHashMap::default()),
            loader: None,
            trace: false,
            open_upvalues: Vec::new(),
            pending_constructors: Vec::new(),
            pending_setters: Vec::new(),
            vm_suspend: None,
            gen_channel: None,
            deferred_tasks: FxHashMap::default(),
            module_exports: FxHashMap::default(),
            opcode_counts: None,
            profile_counters: None,
            proto_constants: FxHashMap::default(),
            no_jit: false,
            linker: Linker::new(),
        };

        if fresh {
            ctx.init_intrinsics();
            ctx.preload_strings();
        }
        ctx
    }

    fn preload_strings(&mut self) {
        const COMMON_STRINGS: &[&str] = &[
            "PASSED", "FAILED", "error", "message", "value", "result",
            "length", "name", "type", "ok", "err", "true", "false",
            "toString", "valueOf", "constructor",
        ];
        for s in COMMON_STRINGS {
            self.heap.alloc_str_interned(s);
        }
    }

    fn init_intrinsics(&mut self) {
        let names = [
            "Array",
            "str",
            "int",
            "float",
            "decimal",
            "bool",
            "char",
            "Map",
            "Set",
            "Range",
            "Error",
            "TypeError",
            "RangeError",
        ];
        for name in names {
            if let Some(nv) = self.globals.get_by_name(name) {
                if let Some(obj) = self.heap.get(nv.as_heap_idx()) {
                    match obj {
                        crate::heap::HeapObj::Class(cls) => {
                            let cls = cls.clone();
                            self.heap.set_intrinsic_class(name, cls);
                        }
                        crate::heap::HeapObj::NativeFn(_, f) => {
                            let f = *f;
                            if let Ok(class_nv) = (f)(self as &mut dyn NativeCtx, &[]) {
                                if let Some(crate::heap::HeapObj::Class(cls)) =
                                    self.heap.get(class_nv.as_heap_idx())
                                {
                                    let cls = cls.clone();
                                    self.heap.set_intrinsic_class(name, cls);
                                    self.globals.set_by_name(name, class_nv);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    pub fn fork_for_task(&self) -> Self {
        Self {
            stack: Vec::with_capacity(16384),
            frames: Vec::with_capacity(512),
            globals: self.globals.clone(),
            heap: self.heap.clone(),
            try_handlers: Vec::new(),
            modules: self.modules.clone(),
            precompiled: Rc::clone(&self.precompiled),
            loader: self.loader.clone(),
            trace: self.trace,
            open_upvalues: Vec::new(),
            pending_constructors: Vec::new(),
            pending_setters: Vec::new(),
            vm_suspend: None,
            gen_channel: None,
            deferred_tasks: FxHashMap::default(),
            module_exports: FxHashMap::default(),
            opcode_counts: None,
            profile_counters: None,
            proto_constants: FxHashMap::default(),
            no_jit: self.no_jit,
            linker: self.linker.clone_state(),
        }
    }

    pub fn run_minor_gc(&mut self) {
        let stack_len = self.stack.len();

        let mut all_vals: Vec<VmValue> = Vec::with_capacity(
            stack_len + self.globals.values.len() + self.modules.len() + self.module_exports.len(),
        );

        all_vals.extend_from_slice(&self.stack);
        let globals_start = all_vals.len();
        all_vals.extend_from_slice(&self.globals.values);
        let modules_start = all_vals.len();
        for v in self.modules.values() {
            all_vals.push(*v);
        }
        let module_exports_start = all_vals.len();
        for v in self.module_exports.values() {
            all_vals.push(*v);
        }

        self.heap.minor_gc(&mut all_vals, &[]);

        self.stack.copy_from_slice(&all_vals[..stack_len]);

        let globals_slice = &all_vals[globals_start..modules_start];
        self.globals.values.copy_from_slice(globals_slice);

        {
            let mut mi = modules_start;
            for v in self.modules.values_mut() {
                *v = all_vals[mi];
                mi += 1;
            }
        }

        {
            let mut ei = module_exports_start;
            for v in self.module_exports.values_mut() {
                *v = all_vals[ei];
                ei += 1;
            }
        }
    }

    pub fn trigger_gc(&mut self) {
        let mut roots: Vec<u32> = Vec::with_capacity(256);
        for v in &self.stack {
            if v.is_heap() {
                roots.push(v.as_heap_idx());
            }
        }
        for v in &self.globals.values {
            if v.is_heap() {
                roots.push(v.as_heap_idx());
            }
        }

        for (_k, v) in &self.modules {
            if v.is_heap() {
                roots.push(v.as_heap_idx());
            }
        }
        for frame in &self.frames {
            for c in frame.closure.constants.iter() {
                if c.is_heap() {
                    roots.push(c.as_heap_idx());
                }
            }
        }
        let _ = self.heap.collect(&roots);
    }
}
