use std::collections::BinaryHeap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use varn_types::{value::SendValue, HostPromise};

struct TimerEntry {
    deadline: Instant,
    seq: u64,
    task: HostPromise,
}

impl PartialEq for TimerEntry {
    fn eq(&self, other: &Self) -> bool {
        self.seq == other.seq
    }
}

impl Eq for TimerEntry {}

impl PartialOrd for TimerEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for TimerEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.deadline
            .cmp(&other.deadline)
            .then(self.seq.cmp(&other.seq))
    }
}

struct WheelState {
    queue: BinaryHeap<std::cmp::Reverse<TimerEntry>>,
    seq: u64,
}

static WHEEL: OnceLock<Mutex<WheelState>> = OnceLock::new();
static WAKER: OnceLock<fn()> = OnceLock::new();

fn wheel() -> &'static Mutex<WheelState> {
    WHEEL.get_or_init(|| {
        Mutex::new(WheelState {
            queue: BinaryHeap::new(),
            seq: 0,
        })
    })
}

pub fn set_waker(f: fn()) {
    let _ = WAKER.set(f);
}

fn poke() {
    if let Some(w) = WAKER.get() {
        w();
    }
}

pub fn sleep_task(ms: u64) -> HostPromise {
    if ms == 0 {
        return HostPromise::resolved(SendValue::Null);
    }
    let task = HostPromise::pending();
    let deadline = Instant::now() + Duration::from_millis(ms);
    let becomes_earliest = {
        let mut guard = wheel().lock().unwrap();
        let seq = guard.seq;
        guard.seq = guard.seq.wrapping_add(1);
        let earliest = guard
            .queue
            .peek()
            .is_none_or(|head| deadline < head.0.deadline);
        guard.queue.push(std::cmp::Reverse(TimerEntry {
            deadline,
            seq,
            task: task.clone(),
        }));
        earliest
    };
    if becomes_earliest {
        poke();
    }
    task
}

pub fn next_deadline() -> Option<Instant> {
    wheel()
        .lock()
        .unwrap()
        .queue
        .peek()
        .map(|entry| entry.0.deadline)
}

pub fn take_due() -> Vec<HostPromise> {
    let mut due = Vec::new();
    let mut guard = wheel().lock().unwrap();
    let now = Instant::now();
    while let Some(entry) = guard.queue.peek() {
        if entry.0.deadline > now && entry.0.task.is_pending() {
            break;
        }
        let entry = guard.queue.pop().unwrap().0;
        if entry.task.is_pending() {
            due.push(entry.task);
        } else {
            PURGED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }
    due
}

static PURGED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static CANCELLED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn purged() -> u64 {
    PURGED.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn note_cancel() {
    if CANCELLED
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        .is_multiple_of(1024)
    {
        let mut guard = wheel().lock().unwrap();
        let before = guard.queue.len();
        guard.queue.retain(|entry| entry.0.task.is_pending());
        PURGED.fetch_add(
            (before - guard.queue.len()) as u64,
            std::sync::atomic::Ordering::Relaxed,
        );
    }
}
