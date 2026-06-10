pub mod assembler;
pub(crate) mod codegen;
pub mod compiler;
pub mod mem;
pub mod regalloc;
pub mod registers;
pub mod safepoint;

use std::any::Any;
use std::rc::Rc;
use varn_types::FunctionProto;
use varn_types::VmValue;

pub type JitFn = unsafe extern "C" fn(
    ctx: *mut std::ffi::c_void,
    closure: *const std::ffi::c_void,
    base: usize,
    exec_ctx: *mut std::ffi::c_void,
) -> VmValue;

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct JitHelpers {
    pub load_const: usize,
    pub load_global_idx: usize,
    pub store_global_idx: usize,
    pub define_global_idx: usize,
    pub eq: usize,
    pub neq: usize,
    pub lt: usize,
    pub lte: usize,
    pub gt: usize,
    pub gte: usize,
    pub add: usize,
    pub sub: usize,
    pub mul: usize,
    pub div: usize,
    pub modulo: usize,
    pub pow: usize,
    pub to_string: usize,
    pub load_global: usize,
    pub load_upvalue: usize,
    pub store_upvalue: usize,
    pub make_closure: usize,
    pub call: usize,
    pub call_method: usize,
    pub get_property: usize,
    pub set_property: usize,
    pub build_array: usize,
    pub build_str: usize,
    pub negate: usize,
    pub logical_not: usize,
    pub get_index: usize,
    pub set_index: usize,
    pub typeof_val: usize,
    pub instanceof: usize,
    pub array_length: usize,
    pub array_push: usize,
    pub array_pop: usize,
    pub array_extend: usize,
    pub str_concat: usize,
    pub str_slice: usize,
    pub str_length: usize,
    pub bit_and: usize,
    pub bit_or: usize,
    pub bit_xor: usize,
    pub shl: usize,
    pub shr: usize,
    pub ushr: usize,
    pub load_module: usize,
    pub load_module_slot: usize,
    pub store_module_slot: usize,
    pub build_object_with_shape: usize,
    pub range: usize,
    pub assert_not_null: usize,
    pub close_upvalue: usize,
    pub get_enum_tag: usize,
    pub is_array: usize,
    pub wrap_spread: usize,
    pub object_keys: usize,
    pub op_in: usize,
    pub object_merge: usize,
    pub get_fixed_field: usize,
    pub set_fixed_field: usize,
    pub get_property_maybe: usize,
    pub get_super: usize,
    pub get_symbol: usize,
    pub bind_method: usize,
    pub define_global: usize,
    pub store_global: usize,
    pub declare_field: usize,
    pub make_class: usize,
    pub inherit: usize,
    pub class_member_op: usize,
    pub build_object: usize,
    pub object_rest: usize,
    pub make_enum_variant: usize,
    pub spawn: usize,
    pub call_spread: usize,
    pub load_module_by_idx: usize,
    pub invoke_virtual: usize,
    pub try_push: usize,
    pub try_pop: usize,
    pub throw: usize,
    pub await_helper: usize,
    pub yield_helper: usize,
    pub get_property_ic_fast: usize,
    pub get_property_maybe_ic_fast: usize,
    pub jit_prepare_call: usize,
    pub jit_post_call: usize,
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

use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone)]
pub struct JitStatsSnapshot {
    pub compile_success: u64,
    pub compile_fail: u64,
    pub total_compile_time_ns: u64,
    pub total_code_size_bytes: u64,
    pub jit_runs: u64,
    pub jit_cached: u64,
    pub interp_runs: u64,
}

pub struct JitStats {
    pub compile_success: AtomicU64,
    pub compile_fail: AtomicU64,
    pub total_compile_time_ns: AtomicU64,
    pub total_code_size_bytes: AtomicU64,
    pub jit_runs: AtomicU64,

    pub jit_cached: AtomicU64,
    pub interp_runs: AtomicU64,
}

impl JitStats {
    pub const fn new() -> Self {
        Self {
            compile_success: AtomicU64::new(0),
            compile_fail: AtomicU64::new(0),
            total_compile_time_ns: AtomicU64::new(0),
            total_code_size_bytes: AtomicU64::new(0),
            jit_runs: AtomicU64::new(0),
            jit_cached: AtomicU64::new(0),
            interp_runs: AtomicU64::new(0),
        }
    }

    pub fn reset(&self) {
        self.compile_success.store(0, Ordering::Relaxed);
        self.compile_fail.store(0, Ordering::Relaxed);
        self.total_compile_time_ns.store(0, Ordering::Relaxed);
        self.total_code_size_bytes.store(0, Ordering::Relaxed);
        self.jit_runs.store(0, Ordering::Relaxed);
        self.jit_cached.store(0, Ordering::Relaxed);
        self.interp_runs.store(0, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> JitStatsSnapshot {
        JitStatsSnapshot {
            compile_success: self.compile_success.load(Ordering::Relaxed),
            compile_fail: self.compile_fail.load(Ordering::Relaxed),
            total_compile_time_ns: self.total_compile_time_ns.load(Ordering::Relaxed),
            total_code_size_bytes: self.total_code_size_bytes.load(Ordering::Relaxed),
            jit_runs: self.jit_runs.load(Ordering::Relaxed),
            jit_cached: self.jit_cached.load(Ordering::Relaxed),
            interp_runs: self.interp_runs.load(Ordering::Relaxed),
        }
    }
}

pub static JIT_STATS: JitStats = JitStats::new();

pub fn compile(
    proto: &FunctionProto,
    constants: &[VmValue],
    helpers: JitHelpers,
) -> Result<(JitFn, Rc<dyn Any>), String> {
    if proto.chunk.code.len() > 250 {
        return Err("JIT Bailout: function too large".to_owned());
    }
    let start = std::time::Instant::now();
    let res = compiler::compile_proto(proto, constants, helpers);
    let elapsed = start.elapsed().as_nanos() as u64;

    match res {
        Ok(jit_buf) => {
            JIT_STATS.compile_success.fetch_add(1, Ordering::Relaxed);
            JIT_STATS
                .total_compile_time_ns
                .fetch_add(elapsed, Ordering::Relaxed);
            JIT_STATS
                .total_code_size_bytes
                .fetch_add(jit_buf.size() as u64, Ordering::Relaxed);

            let entry_ptr = jit_buf.as_ptr();

            let jit_fn: JitFn = unsafe { std::mem::transmute(entry_ptr) };

            let jit_code = Rc::new(jit_buf) as Rc<dyn Any>;

            Ok((jit_fn, jit_code))
        }
        Err(e) => {
            JIT_STATS.compile_fail.fetch_add(1, Ordering::Relaxed);
            JIT_STATS
                .total_compile_time_ns
                .fetch_add(elapsed, Ordering::Relaxed);
            Err(e)
        }
    }
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct JitCallArgs {
    pub callee: VmValue,
    pub arg_start: usize,
    pub arg_count: usize,
    pub dest: usize,
    pub ip: usize,
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct JitCallMethodArgs {
    pub this_val: VmValue,
    pub name_idx: usize,
    pub cs: usize,
    pub arg_start: usize,
    pub arg_count: usize,
    pub dest: usize,
    pub ip: usize,
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct JitGetPropertyArgs {
    pub obj: VmValue,
    pub name_idx: usize,
    pub cs_idx: usize,
    pub dest: usize,
    pub ip: usize,
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
