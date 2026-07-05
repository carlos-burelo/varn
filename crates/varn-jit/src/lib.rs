pub mod assembler;
pub(crate) mod codegen;
pub mod compiler;
pub(crate) mod loop_hoist;
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

/// Byte offsets and probed layout facts that let emitted code walk from a
/// heap-tagged `VmValue` to an array element without any FFI call:
///
/// `[ExecCtx + heap_field] → RcBox → HeapInner.objects (Vec words) → slot
/// (Option<HeapObj>, tag byte + payload Rc) → RcBox → Vec<VmValue> words →
/// data[idx]`.
///
/// Rust does not guarantee `Vec`'s field order, so the ptr/len word offsets
/// are PROBED at startup against vectors with known contents — stable for
/// the lifetime of one binary, which is exactly the lifetime of any JIT
/// code that embeds them.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct JitArrayLayout {
    /// RcBox base → the old-gen `objects` Vec's three words inside HeapInner.
    pub slots_vec_off: usize,
    /// RcBox base → the nursery's `objects` Vec's three words. Heap indices
    /// use bit 31 to distinguish old gen (set) from nursery (clear).
    pub nursery_slots_vec_off: usize,
    /// Word offset of the data pointer inside `Vec<Option<HeapObj>>`.
    pub slots_ptr_off: usize,
    /// `size_of::<Option<HeapObj>>()` — slot stride.
    pub slot_size: usize,
    /// Discriminant byte value of `HeapObj::Array` (niche-shared by the
    /// `Option` wrapper).
    pub array_tag: usize,
    /// Slot base → the array payload's Rc pointer.
    pub payload_off: usize,
    /// Word offsets of (data ptr, len) inside `Vec<VmValue>`.
    pub elems_ptr_off: usize,
    pub elems_len_off: usize,
}

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
    pub load_static_fn: usize,
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
    pub jit_array_get_fast: usize,
    pub jit_array_set_fast: usize,
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
    pub jit_push_self_frame: usize,
    pub jit_post_call: usize,
    pub jit_ensure_stack_capacity: usize,
    pub dispatch_intrinsic: usize,
    pub jit_is_native_fn: usize,
    pub jit_call_native_fast: usize,
    pub jit_call_native_op: usize,
    /// `extern "C" fn(*mut ExecCtx, fn_addr, args_start, total)` — direct
    /// native call with the function pointer already resolved.
    pub jit_call_native_fnptr: usize,
    /// Compile-time op-id → native fn address resolution (0 = unknown).
    /// Lets `CallNativeOp` embed the target directly instead of paying a
    /// hash lookup on every runtime call.
    pub resolve_native_op: fn(u64) -> usize,
    /// Probed heap/array layout for the inline array-read fast path.
    pub array_layout: JitArrayLayout,
    pub open_upvalues_offset: usize,
    pub pending_constructors_offset: usize,
    /// `extern "C" fn(*mut ExecCtx)` — loop back-edge GC safepoint.
    pub gc_safepoint: usize,
    /// Byte offset of the heap field (an Rc, i.e. one pointer) inside ExecCtx.
    pub heap_field_offset: usize,
    /// Byte offset from the heap RcBox pointer to the nursery live-object count.
    pub nursery_len_offset: usize,
    /// Nursery fill level at which the safepoint must run.
    pub nursery_threshold: usize,
    pub jit_native_result_offset: usize,
    pub globals_offset: usize,
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
