use std::cell::Cell;
use std::time::{Duration, Instant};

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

    pub fn start(&self) {
        self.start.set(Instant::now());
    }

    pub fn stop(&self) {
        let dur = self.start.get().elapsed();
        self.elapsed.set(self.elapsed.get() + dur);
    }

    pub fn elapsed(&self) -> Duration {
        self.elapsed.get()
    }
}

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
