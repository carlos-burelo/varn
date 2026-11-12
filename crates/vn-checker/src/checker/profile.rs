use std::time::{Duration, Instant};

pub(super) fn timed<T>(slot: &mut Duration, f: impl FnOnce() -> T) -> T {
    let started = Instant::now();
    let value = f();
    *slot = started.elapsed();
    value
}
