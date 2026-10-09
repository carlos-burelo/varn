use std::sync::atomic::{AtomicU64, Ordering};

static PASSED: AtomicU64 = AtomicU64::new(0);
static FAILED: AtomicU64 = AtomicU64::new(0);

pub fn reset_testing_counters() {
    PASSED.store(0, Ordering::Relaxed);
    FAILED.store(0, Ordering::Relaxed);
}

pub fn inc_passed() {
    PASSED.fetch_add(1, Ordering::Relaxed);
}

pub fn inc_failed() {
    FAILED.fetch_add(1, Ordering::Relaxed);
}

pub fn print_summary() {
    let passed = PASSED.load(Ordering::Relaxed);
    let failed = FAILED.load(Ordering::Relaxed);
    println!("\n════════════════════════════════════════");
    println!("PASSED: {passed}");
    println!("FAILED: {failed}");
    if failed == 0 {
        println!("ALL TESTS PASSED");
    } else {
        println!("SOME TESTS FAILED");
    }
}

pub fn summary(
    ctx: &mut dyn varn_types::NativeCtx,
    _args: &[varn_types::VmValue],
) -> Result<varn_types::VmValue, String> {
    if !ctx.print_silent() {
        print_summary();
    }
    Ok(varn_types::VmValue::null())
}
