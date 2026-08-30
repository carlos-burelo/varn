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
    ///
    /// Owned rather than `&'static str` so the reason can name the threshold
    /// that actually fired: the literal it used to carry said "> 250 words"
    /// long after [`crate::SIZE_GATE_WORDS`] became 8192, which is a report
    /// that misdescribes the build it came from.
    Gated(String),
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
    /// Frames rescued mid-flight by on-stack replacement. Deliberately NOT
    /// part of `jit_runs`: such a frame was already counted as an interpreted
    /// ENTRY, and it is the same frame — adding it again would make
    /// `total_frames` count one activation twice. It is the answer to "did OSR
    /// fire", not to "how many frames ran compiled".
    pub osr_entries: u64,
}

impl JitStatsSnapshot {
    /// Frame entries that ENTERED through compiled code.
    ///
    /// Not the same question as "did this frame run as machine code": a frame
    /// rescued by OSR entered on the interpreter and finished compiled, and it
    /// is counted in `interp_runs`, not here. Use [`Self::machine_code_frames`]
    /// for coverage and this one only where the ENTRY tier is the subject.
    pub fn clif_frames(&self) -> u64 {
        self.jit_runs
    }

    /// Total frame entries observed, compiled plus interpreted.
    pub fn total_frames(&self) -> u64 {
        self.jit_runs + self.interp_runs
    }

    /// Frame entries that executed machine code at all: entered compiled, or
    /// entered interpreted and rescued mid-loop by on-stack replacement.
    ///
    /// This is the numerator every coverage figure wants. Counting only
    /// `jit_runs` reports a program whose hot loops all run compiled as 0%
    /// covered — measured on `bench_str_ops.vn`, where the headline said
    /// "0 de 62 frames (0.0%)" while removing OSR (`VARN_JIT_OSR=4000000000`)
    /// took `char_code` from 12.6 ms to 134.4 ms. Adding `osr_entries` to
    /// `jit_runs` does NOT double-count the activation, because
    /// [`Self::total_frames`] keeps counting it once, on the `interp_runs`
    /// side: the two numerators partition the same denominator with
    /// [`Self::never_compiled_frames`].
    pub fn machine_code_frames(&self) -> u64 {
        self.jit_runs + self.osr_entries
    }

    /// Frame entries that ran to completion on the interpreter — the ones a
    /// coverage report should flag. The complement of
    /// [`Self::machine_code_frames`] over [`Self::total_frames`].
    pub fn never_compiled_frames(&self) -> u64 {
        self.interp_runs.saturating_sub(self.osr_entries)
    }

    /// Share of frame entries that entered through compiled code, 0.0..=1.0.
    /// See [`Self::clif_frames`] for why this is not the coverage figure.
    pub fn clif_frame_ratio(&self) -> f64 {
        self.frame_share_of(self.jit_runs)
    }

    /// Share of frame entries that executed machine code, 0.0..=1.0.
    pub fn machine_code_ratio(&self) -> f64 {
        self.frame_share_of(self.machine_code_frames())
    }

    /// Share of frame entries that never left the interpreter, 0.0..=1.0.
    /// This is what an opcode or VM breakdown must be scaled by: attributing
    /// interpreter counters to a run needs the frames that really stayed
    /// interpreted, not every frame that merely started that way.
    pub fn never_compiled_ratio(&self) -> f64 {
        self.frame_share_of(self.never_compiled_frames())
    }

    /// `n` as a share of all observed frame entries. 0.0 when nothing ran, so
    /// that an empty snapshot reports "nothing covered" rather than dividing
    /// by zero. Public because the coverage table shares this denominator
    /// across rows the named ratios above do not cover.
    pub fn frame_share_of(&self, n: u64) -> f64 {
        let total = self.total_frames();
        if total == 0 {
            return 0.0;
        }
        n as f64 / total as f64
    }

    /// Functions the JIT was asked about: routed, gated, or bailed.
    pub fn functions_seen(&self) -> u64 {
        self.compile_success + self.compile_fail + self.gate_rejected
    }

    /// Fraction of offered functions that were successfully compiled, 0.0..=1.0.
    ///
    /// This is the only frame-count-independent coverage figure: it counts each
    /// unique function once regardless of how many times it was called, and it
    /// is not affected by JIT-direct call paths that bypass the dispatcher.
    /// Frame-based ratios (`machine_code_ratio`) undercount compiled coverage
    /// for any function whose recursive or chained calls run as machine-code
    /// without re-entering the interpreter trampoline.
    pub fn fn_compilation_rate(&self) -> f64 {
        let seen = self.functions_seen();
        if seen == 0 {
            return 1.0;
        }
        self.compile_success as f64 / seen as f64
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
