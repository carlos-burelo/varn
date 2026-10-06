













use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

static ENABLED: AtomicBool = AtomicBool::new(false);



pub fn init() {
    ENABLED.store(
        std::env::var_os("VARN_GC_TRACE").is_some(),
        Ordering::Relaxed,
    );
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
pub fn note_end(before: Before, collection_no: u64, promoted_after: u64) {
    if !enabled() {
        return;
    }
    let elapsed = before.t0.elapsed();
    let promoted_this = promoted_after - before.promoted_before;
    let reclaimed = before.objects_before.saturating_sub(promoted_this as usize);
    eprintln!(
        "[gc] minor #{collection_no}: in={} promoted={promoted_this} reclaimed={reclaimed} took={:.3}ms",
        before.objects_before,
        elapsed.as_secs_f64() * 1000.0,
    );
}
