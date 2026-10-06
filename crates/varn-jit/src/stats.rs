use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompileOutcome {
    Routed,

    Gated(String),

    Bailed(String),
}

impl CompileOutcome {
    pub fn is_routed(&self) -> bool {
        matches!(self, CompileOutcome::Routed)
    }

    pub fn reason(&self) -> Option<&str> {
        match self {
            CompileOutcome::Routed => None,
            CompileOutcome::Gated(r) => Some(r),
            CompileOutcome::Bailed(r) => Some(r),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CompileRecord {
    pub name: String,

    pub words: usize,
    pub outcome: CompileOutcome,

    pub compile_ns: u64,
}

#[derive(Debug, Clone)]
pub struct JitStatsSnapshot {
    pub compile_success: u64,
    pub compile_fail: u64,

    pub gate_rejected: u64,
    pub total_compile_time_ns: u64,

    pub backend_time_ns: u64,
    pub total_code_size_bytes: u64,
    pub jit_runs: u64,
    pub jit_cached: u64,
    pub interp_runs: u64,

    pub osr_entries: u64,
}

impl JitStatsSnapshot {
    pub fn total_frames(&self) -> u64 {
        self.jit_runs + self.interp_runs
    }

    pub fn machine_code_frames(&self) -> u64 {
        self.jit_runs + self.osr_entries
    }

    pub fn never_compiled_frames(&self) -> u64 {
        self.interp_runs.saturating_sub(self.osr_entries)
    }

    pub fn machine_code_ratio(&self) -> f64 {
        self.frame_share_of(self.machine_code_frames())
    }

    pub fn never_compiled_ratio(&self) -> f64 {
        self.frame_share_of(self.never_compiled_frames())
    }

    pub fn frame_share_of(&self, n: u64) -> f64 {
        let total = self.total_frames();
        if total == 0 {
            return 0.0;
        }
        n as f64 / total as f64
    }

    pub fn functions_seen(&self) -> u64 {
        self.compile_success + self.compile_fail + self.gate_rejected
    }

    pub fn fn_compilation_rate(&self) -> f64 {
        let seen = self.functions_seen();
        if seen == 0 {
            return 1.0;
        }
        self.compile_success as f64 / seen as f64
    }

    pub fn ns_per_routed_fn(&self) -> Option<f64> {
        if self.compile_success == 0 {
            return None;
        }
        Some(self.total_compile_time_ns as f64 / self.compile_success as f64)
    }
}

pub struct JitStats {
    pub compile_success: AtomicU64,
    pub compile_fail: AtomicU64,
    pub gate_rejected: AtomicU64,
    pub total_compile_time_ns: AtomicU64,
    pub backend_time_ns: AtomicU64,
    pub total_code_size_bytes: AtomicU64,
    pub jit_runs: AtomicU64,
    pub jit_cached: AtomicU64,
    pub interp_runs: AtomicU64,
    pub osr_entries: AtomicU64,
}

impl JitStats {
    pub const fn new() -> Self {
        Self {
            compile_success: AtomicU64::new(0),
            compile_fail: AtomicU64::new(0),
            gate_rejected: AtomicU64::new(0),
            total_compile_time_ns: AtomicU64::new(0),
            backend_time_ns: AtomicU64::new(0),
            total_code_size_bytes: AtomicU64::new(0),
            jit_runs: AtomicU64::new(0),
            jit_cached: AtomicU64::new(0),
            interp_runs: AtomicU64::new(0),
            osr_entries: AtomicU64::new(0),
        }
    }

    pub fn reset(&self) {
        self.compile_success.store(0, Ordering::Relaxed);
        self.compile_fail.store(0, Ordering::Relaxed);
        self.gate_rejected.store(0, Ordering::Relaxed);
        self.total_compile_time_ns.store(0, Ordering::Relaxed);
        self.backend_time_ns.store(0, Ordering::Relaxed);
        self.total_code_size_bytes.store(0, Ordering::Relaxed);
        self.jit_runs.store(0, Ordering::Relaxed);
        self.jit_cached.store(0, Ordering::Relaxed);
        self.interp_runs.store(0, Ordering::Relaxed);
        self.osr_entries.store(0, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> JitStatsSnapshot {
        JitStatsSnapshot {
            compile_success: self.compile_success.load(Ordering::Relaxed),
            compile_fail: self.compile_fail.load(Ordering::Relaxed),
            gate_rejected: self.gate_rejected.load(Ordering::Relaxed),
            total_compile_time_ns: self.total_compile_time_ns.load(Ordering::Relaxed),
            backend_time_ns: self.backend_time_ns.load(Ordering::Relaxed),
            total_code_size_bytes: self.total_code_size_bytes.load(Ordering::Relaxed),
            jit_runs: self.jit_runs.load(Ordering::Relaxed),
            jit_cached: self.jit_cached.load(Ordering::Relaxed),
            interp_runs: self.interp_runs.load(Ordering::Relaxed),
            osr_entries: self.osr_entries.load(Ordering::Relaxed),
        }
    }
}

impl Default for JitStats {
    fn default() -> Self {
        Self::new()
    }
}

pub static JIT_STATS: JitStats = JitStats::new();

static RECORDING: AtomicBool = AtomicBool::new(false);
static RECORDS: Mutex<Vec<CompileRecord>> = Mutex::new(Vec::new());

pub fn start_recording() {
    if let Ok(mut buf) = RECORDS.lock() {
        buf.clear();
    }
    RECORDING.store(true, Ordering::Relaxed);
}

pub fn take_records() -> Vec<CompileRecord> {
    RECORDING.store(false, Ordering::Relaxed);
    RECORDS
        .lock()
        .map(|mut buf| std::mem::take(&mut *buf))
        .unwrap_or_default()
}

pub(crate) fn record(make: impl FnOnce() -> CompileRecord) {
    if !RECORDING.load(Ordering::Relaxed) {
        return;
    }
    if let Ok(mut buf) = RECORDS.lock() {
        buf.push(make());
    }
}
