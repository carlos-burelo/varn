use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};

#[derive(Default)]
struct Pending {
    tokens: VecDeque<u64>,
    generation: u64,
}

pub struct WakeQueue {
    pending: Mutex<Pending>,
    changed: Condvar,
}

impl WakeQueue {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            pending: Mutex::new(Pending::default()),
            changed: Condvar::new(),
        })
    }

    pub fn push(&self, token: u64) {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        pending.tokens.push_back(token);
        pending.generation = pending.generation.wrapping_add(1);
        drop(pending);
        self.changed.notify_all();
    }

    pub fn generation(&self) -> u64 {
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .generation
    }

    pub fn drain_into(&self, out: &mut VecDeque<u64>) {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        out.append(&mut pending.tokens);
    }

    pub fn wait_past(&self, entered: u64, done: impl Fn() -> bool) {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        while !done() && pending.generation == entered {
            pending = self.changed.wait(pending).unwrap();
        }
    }
}

#[derive(Clone)]
pub struct WakeToken {
    queue: Arc<WakeQueue>,
    token: u64,
}

impl WakeToken {
    pub fn new(queue: &Arc<WakeQueue>, token: u64) -> Self {
        Self {
            queue: Arc::clone(queue),
            token,
        }
    }

    pub fn fire(self) {
        self.queue.push(self.token);
    }
}
