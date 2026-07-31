//! Compilation telemetry.
//!
//! Two layers, deliberately separate:
//!
//! * [`JIT_STATS`] — always-on process counters. Cheap relaxed atomics, read by
//!   the bench headline.
//! * [`CompileRecord`] — opt-in per-function detail. Off by default; `vn bench
//!   -v` and `vn debug -p tiers` switch it on around a single run.
//!
//! The distinction that makes the coverage number honest lives in
//! [`CompileOutcome`]: a function rejected by a gate in `varn_jit::compile`
//! never reaches Cranelift at all, so it shows up in neither `compile_fail` nor
//! any `CLIF BAIL` trace. Counting only lowering bails reports "0 bails" while
//! functions silently run interpreted.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

/// What happened when the JIT was asked to compile one function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompileOutcome {
    /// Lowered to machine code and installed.
    Routed,
    /// Rejected by a gate *before* Cranelift was asked. Carries the gate's
    /// reason; these are invisible to `compile_fail` and to `CLIF BAIL`.
    Gated(&'static str),
    /// Cranelift was asked and refused to lower it.
    Bailed(String),
}

impl CompileOutcome {
    pub fn is_routed(&self) -> bool {
        matches!(self, CompileOutcome::Routed)
    }

    /// Short reason text, or `None` when routed.
    pub fn reason(&self) -> Option<&str> {
        match self {
            CompileOutcome::Routed => None,
            CompileOutcome::Gated(r) => Some(r),
            CompileOutcome::Bailed(r) => Some(r),
        }
    }
}

/// One function's compilation attempt.
#[derive(Debug, Clone)]
pub struct CompileRecord {
    pub name: String,
    /// Bytecode length in words — the quantity the size gate tests.
    pub words: usize,
    pub outcome: CompileOutcome,
    /// Zero for gated functions: no lowering ran.
    pub compile_ns: u64,
    /// Zero unless routed.
    pub code_bytes: u64,
}

#[derive(Debug, Clone)]
pub struct JitStatsSnapshot {
    pub compile_success: u64,
    pub compile_fail: u64,
    /// Functions turned away by a gate before Cranelift saw them.
    pub gate_rejected: u64,
    pub total_compile_time_ns: u64,
    /// Of `total_compile_time_ns`, the part spent inside Cranelift's own
    /// `Context::compile`. The remainder is our lowering: walking bytecode and
    /// building CLIF IR. Splitting them says which side a compile-time fix
    /// belongs on.
    pub backend_time_ns: u64,
    pub total_code_size_bytes: u64,
    pub jit_runs: u64,
    pub jit_cached: u64,
    pub interp_runs: u64,
}

impl JitStatsSnapshot {
    /// Frame entries that ran compiled code, cached or freshly lowered.
    pub fn clif_frames(&self) -> u64 {
        self.jit_runs
    }

    /// Total frame entries observed, compiled plus interpreted.
    pub fn total_frames(&self) -> u64 {
        self.jit_runs + self.interp_runs
    }

    /// Share of frame entries that ran as machine code, 0.0..=1.0.
    pub fn clif_frame_ratio(&self) -> f64 {
        let total = self.total_frames();
        if total == 0 {
            return 0.0;
        }
        self.jit_runs as f64 / total as f64
    }

    /// Functions the JIT was asked about: routed, gated, or bailed.
    pub fn functions_seen(&self) -> u64 {
        self.compile_success + self.compile_fail + self.gate_rejected
    }

    /// Mean lowering cost per successfully routed function.
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

/// Start collecting per-function records, discarding anything held from a
/// previous window. Pair with [`take_records`].
pub fn start_recording() {
    if let Ok(mut buf) = RECORDS.lock() {
        buf.clear();
    }
    RECORDING.store(true, Ordering::Relaxed);
}

/// Stop collecting and hand back what was gathered.
pub fn take_records() -> Vec<CompileRecord> {
    RECORDING.store(false, Ordering::Relaxed);
    RECORDS
        .lock()
        .map(|mut buf| std::mem::take(&mut *buf))
        .unwrap_or_default()
}

/// Append one record when recording is active. The `AtomicBool` check is the
/// only cost paid on the production path.
pub(crate) fn record(make: impl FnOnce() -> CompileRecord) {
    if !RECORDING.load(Ordering::Relaxed) {
        return;
    }
    if let Ok(mut buf) = RECORDS.lock() {
        buf.push(make());
    }
}
