use std::time::{Duration, Instant};
use std::sync::Mutex;


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

    
    pub fn stop(&self) {
        let dur = self.start.elapsed();
        let mut guard = self.elapsed.lock().unwrap();
        *guard += dur;
    }

    
    pub fn elapsed(&self) -> Duration {
        *self.elapsed.lock().unwrap()
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


