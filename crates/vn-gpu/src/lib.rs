mod arena;
mod backend;
pub(crate) mod execute;
mod graph;

pub use arena::{pool, Bufs, Handle};
pub use backend::{adapter_name, backend_name, device_count, is_discrete, probe};
pub use graph::Op;

pub(crate) const GPU_MIN_MACS: usize = 200_000;

static LAST_ENGINE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

pub(crate) fn mark_cpu() {
    LAST_ENGINE.store(0, std::sync::atomic::Ordering::Relaxed);
}

pub(crate) fn mark_gpu() {
    LAST_ENGINE.store(1, std::sync::atomic::Ordering::Relaxed);
}

pub fn last_engine() -> &'static str {
    if LAST_ENGINE.load(std::sync::atomic::Ordering::Relaxed) == 1 {
        "gpu"
    } else {
        "cpu"
    }
}
