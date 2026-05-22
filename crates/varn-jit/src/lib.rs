pub mod assembler;
pub mod compiler;
pub mod mem;
pub mod registers;
pub mod safepoint;

use std::any::Any;
use std::rc::Rc;
use varn_types::FunctionProto;
use varn_types::VmValue;

/// The platform-neutral JIT function entry point type.
/// Invokes the compiled native code with the ExecCtx stack, closure, current stack frame base index, and ExecCtx pointer.
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
}

use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone)]
pub struct JitStatsSnapshot {
    pub compile_success: u64,
    pub compile_fail: u64,
    pub total_compile_time_ns: u64,
    pub total_code_size_bytes: u64,
    pub jit_runs: u64,
    pub interp_runs: u64,
}

pub struct JitStats {
    pub compile_success: AtomicU64,
    pub compile_fail: AtomicU64,
    pub total_compile_time_ns: AtomicU64,
    pub total_code_size_bytes: AtomicU64,
    pub jit_runs: AtomicU64,
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
            interp_runs: AtomicU64::new(0),
        }
    }

    pub fn reset(&self) {
        self.compile_success.store(0, Ordering::Relaxed);
        self.compile_fail.store(0, Ordering::Relaxed);
        self.total_compile_time_ns.store(0, Ordering::Relaxed);
        self.total_code_size_bytes.store(0, Ordering::Relaxed);
        self.jit_runs.store(0, Ordering::Relaxed);
        self.interp_runs.store(0, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> JitStatsSnapshot {
        JitStatsSnapshot {
            compile_success: self.compile_success.load(Ordering::Relaxed),
            compile_fail: self.compile_fail.load(Ordering::Relaxed),
            total_compile_time_ns: self.total_compile_time_ns.load(Ordering::Relaxed),
            total_code_size_bytes: self.total_code_size_bytes.load(Ordering::Relaxed),
            jit_runs: self.jit_runs.load(Ordering::Relaxed),
            interp_runs: self.interp_runs.load(Ordering::Relaxed),
        }
    }
}

pub static JIT_STATS: JitStats = JitStats::new();

/// Compiles a bytecode function and returns both the function pointer entry point
/// and the type-erased executable memory buffer (to keep it alive).
pub fn compile(proto: &FunctionProto, helpers: JitHelpers) -> Result<(JitFn, Rc<dyn Any>), String> {
    let start = std::time::Instant::now();
    let res = compiler::compile_proto(proto, helpers);
    let elapsed = start.elapsed().as_nanos() as u64;

    match res {
        Ok(jit_buf) => {
            JIT_STATS.compile_success.fetch_add(1, Ordering::Relaxed);
            JIT_STATS.total_compile_time_ns.fetch_add(elapsed, Ordering::Relaxed);
            JIT_STATS.total_code_size_bytes.fetch_add(jit_buf.size() as u64, Ordering::Relaxed);
            
            let entry_ptr = jit_buf.as_ptr();
            
            // Cast the raw executable pointer to a function pointer
            let jit_fn: JitFn = unsafe { std::mem::transmute(entry_ptr) };
            
            // Wrap the JitBuffer in an Rc<dyn Any> to pass ownership to the VM cleanly
            let jit_code = Rc::new(jit_buf) as Rc<dyn Any>;
            
            Ok((jit_fn, jit_code))
        }
        Err(e) => {
            JIT_STATS.compile_fail.fetch_add(1, Ordering::Relaxed);
            JIT_STATS.total_compile_time_ns.fetch_add(elapsed, Ordering::Relaxed);
            Err(e)
        }
    }
}
