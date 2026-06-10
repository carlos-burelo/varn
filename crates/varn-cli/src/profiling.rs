use std::cell::Cell;
use std::time::{Duration, Instant};

/// Lightweight single-threaded phase timer for benchmarking.
pub struct PhaseTimer {
    start: Cell<Instant>,
    elapsed: Cell<Duration>,
}

impl PhaseTimer {
    pub fn new() -> Self {
        Self {
            start: Cell::new(Instant::now()),
            elapsed: Cell::new(Duration::ZERO),
        }
    }

    /// (Re)start the timer.
    pub fn start(&self) {
        self.start.set(Instant::now());
    }

    /// Stop and accumulate elapsed time since last `start`.
    pub fn stop(&self) {
        let dur = self.start.get().elapsed();
        self.elapsed.set(self.elapsed.get() + dur);
    }

    /// Retrieve the accumulated duration.
    pub fn elapsed(&self) -> Duration {
        self.elapsed.get()
    }
}

/// Timers for the benchmark phases.
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
            read: PhaseTimer::new(),
            lex: PhaseTimer::new(),
            parse: PhaseTimer::new(),
            check: PhaseTimer::new(),
            compile: PhaseTimer::new(),
            optimize: PhaseTimer::new(),
            execute: PhaseTimer::new(),
        }
    }
}
