use std::sync::atomic::{AtomicU32, Ordering};

static COLLECTING: AtomicU32 = AtomicU32::new(0);
static COMPILING: AtomicU32 = AtomicU32::new(0);

pub(super) const COLLECTING_BIT: u8 = 1;
pub(super) const COMPILING_BIT: u8 = 2;

pub(crate) struct Activity(&'static AtomicU32);

impl Activity {
    pub(crate) fn collecting() -> Self {
        COLLECTING.fetch_add(1, Ordering::Relaxed);
        Self(&COLLECTING)
    }

    pub(crate) fn compiling() -> Self {
        COMPILING.fetch_add(1, Ordering::Relaxed);
        Self(&COMPILING)
    }
}

impl Drop for Activity {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

pub(super) fn snapshot() -> u8 {
    let mut bits = 0;
    if COLLECTING.load(Ordering::Relaxed) > 0 {
        bits |= COLLECTING_BIT;
    }
    if COMPILING.load(Ordering::Relaxed) > 0 {
        bits |= COMPILING_BIT;
    }
    bits
}
