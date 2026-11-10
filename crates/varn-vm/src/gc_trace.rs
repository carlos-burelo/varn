use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

static ENABLED: AtomicBool = AtomicBool::new(false);

pub fn configure(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

#[inline(always)]
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

pub struct Before {
    t0: Instant,
    objects_before: usize,
    promoted_before: u64,
}

#[inline]
pub fn note_start(objects_before: usize, promoted_before: u64) -> Before {
    Before {
        t0: Instant::now(),
        objects_before,
        promoted_before,
    }
}

#[inline]
pub fn note_end(
    before: Before,
    collection_no: u64,
    promoted_after: u64,
    cells_bytes: u64,
    external_bytes: u64,
    phases: (std::time::Duration, std::time::Duration),
) {
    if !enabled() {
        return;
    }
    let elapsed = before.t0.elapsed();
    let promoted_this = promoted_after - before.promoted_before;
    let reclaimed = before.objects_before.saturating_sub(promoted_this as usize);
    eprintln!(
        "[gc] minor #{collection_no}: in={} cells={cells_bytes}B external={external_bytes}B promoted={promoted_this} reclaimed={reclaimed} mark={:.3}ms sweep={:.3}ms took={:.3}ms",
        before.objects_before,
        phases.0.as_secs_f64() * 1000.0,
        phases.1.as_secs_f64() * 1000.0,
        elapsed.as_secs_f64() * 1000.0,
    );
}

pub fn major(
    collection_no: u64,
    freed: usize,
    live_bytes: u64,
    threshold: u64,
    took: std::time::Duration,
) {
    if !enabled() {
        return;
    }
    eprintln!(
        "[gc] major #{collection_no}: freed={freed} old={live_bytes}B next={threshold}B took={:.3}ms",
        took.as_secs_f64() * 1000.0,
    );
}
