use std::collections::VecDeque;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Condvar, Mutex, OnceLock, PoisonError};
use std::time::Duration;
use varn_types::task::Completion;
use varn_types::value::SendValue;
use varn_types::HostPromise;

const MAX_WORKERS: usize = 256;
const IDLE_EXIT: Duration = Duration::from_secs(10);

type Job = Box<dyn FnOnce() + Send>;

struct State {
    queue: VecDeque<Job>,
    idle: usize,
    total: usize,
}

struct Pool {
    state: Mutex<State>,
    wake: Condvar,
}

fn pool() -> &'static Pool {
    static POOL: OnceLock<Pool> = OnceLock::new();
    POOL.get_or_init(|| Pool {
        state: Mutex::new(State {
            queue: VecDeque::new(),
            idle: 0,
            total: 0,
        }),
        wake: Condvar::new(),
    })
}

fn next_job(pool: &Pool) -> Option<Job> {
    let mut state = pool.state.lock().unwrap_or_else(PoisonError::into_inner);
    loop {
        if let Some(job) = state.queue.pop_front() {
            return Some(job);
        }
        state.idle += 1;
        let (guard, timeout) = pool
            .wake
            .wait_timeout(state, IDLE_EXIT)
            .unwrap_or_else(PoisonError::into_inner);
        state = guard;
        state.idle -= 1;
        if timeout.timed_out() && state.queue.is_empty() {
            state.total -= 1;
            return None;
        }
    }
}

fn worker() {
    let pool = pool();
    while let Some(job) = next_job(pool) {
        job();
    }
}

pub fn detach(work: impl FnOnce() + Send + 'static) {
    crate::modules::net::driver::driver();
    submit(Box::new(move || {
        let _ = catch_unwind(AssertUnwindSafe(work));
    }));
}

fn submit(job: Job) {
    let pool = pool();
    let spawn = {
        let mut state = pool.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.queue.push_back(job);
        let spawn = state.queue.len() > state.idle && state.total < MAX_WORKERS;
        if spawn {
            state.total += 1;
        }
        spawn
    };
    if spawn {
        std::thread::Builder::new()
            .name("varn-blocking".to_string())
            .spawn(worker)
            .unwrap_or_else(|e| panic!("cannot start a blocking worker: {e}"));
    } else {
        pool.wake.notify_one();
    }
}

pub fn failure(message: impl Into<String>) -> SendValue {
    SendValue::Error {
        class: varn_core::RuntimeErrorKind::Error.class_name().to_string(),
        message: message.into(),
    }
}

pub fn run(work: impl FnOnce() -> Completion + Send + 'static) -> HostPromise {
    let promise = HostPromise::pending();
    let done = promise.clone();
    let rejected = promise.clone();
    detach(move || match catch_unwind(AssertUnwindSafe(work)) {
        Ok(outcome) => done.complete(outcome),
        Err(_) => rejected.complete(Err(failure("blocking operation panicked"))),
    });
    promise
}
