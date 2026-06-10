use std::time::{Duration, Instant};
use std::sync::Mutex;

/// Simple high‑resolution timer for measuring phases of JIT compilation.
pub struct PhaseTimer {
    start: Instant,
    elapsed: Mutex<Duration>,
}

impl PhaseTimer {
    pub fn start() -> Self {
        Self {
            start: Instant::now(),
            elapsed: Mutex::new(Duration::ZERO),
        }
    }

    /// Record the time elapsed since `start` and add it to the accumulated duration.
    pub fn stop(&self) {
        let dur = self.start.elapsed();
        let mut guard = self.elapsed.lock().unwrap();
        *guard += dur;
    }

    /// Retrieve the accumulated duration.
    pub fn elapsed(&self) -> Duration {
        *self.elapsed.lock().unwrap()
    }
}

/// Global timers for the most critical JIT phases.
pub struct JitTimers {
    pub read: PhaseTimer,
    pub lex: PhaseTimer,
    pub parse: PhaseTimer,
    pub check: PhaseTimer,
    pub compile: PhaseTimer,
    pub optimize: PhaseTimer,
    pub execute: PhaseTimer,
}

impl JitTimers {
    pub fn new() -> Self {
        Self {
            read: PhaseTimer::start(),
            lex: PhaseTimer::start(),
            parse: PhaseTimer::start(),
            check: PhaseTimer::start(),
            compile: PhaseTimer::start(),
            optimize: PhaseTimer::start(),
            execute: PhaseTimer::start(),
        }
    }
}

// Helper to format a `Duration` as a CSV‑friendly string.
pub fn fmt_duration_csv(d: Duration) -> String {
    // Output in microseconds with three decimals for readability.
    let micros = d.as_nanos() as f64 / 1_000.0;
    format!("{:.3}", micros)
}
