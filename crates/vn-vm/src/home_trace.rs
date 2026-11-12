use std::sync::atomic::{AtomicBool, Ordering};

static ON: AtomicBool = AtomicBool::new(false);

pub fn configure(enabled: bool) {
    ON.store(enabled, Ordering::Relaxed);
}

#[inline(always)]
pub(crate) fn enabled() -> bool {
    ON.load(Ordering::Relaxed)
}
