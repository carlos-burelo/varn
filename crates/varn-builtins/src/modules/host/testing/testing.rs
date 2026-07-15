use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

static PASSED: AtomicU64 = AtomicU64::new(0);
static FAILED: AtomicU64 = AtomicU64::new(0);
static SILENT: AtomicBool = AtomicBool::new(false);

pub fn set_testing_silent(silent: bool) {
    SILENT.store(silent, Ordering::Relaxed);
}

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
    if !SILENT.load(Ordering::Relaxed) {
        println!("\n════════════════════════════════════════");
        println!("PASSED: {passed}");
        println!("FAILED: {failed}");
        if failed == 0 {
            println!("ALL TESTS PASSED");
        } else {
            println!("SOME TESTS FAILED");
        }
    }
}

pub fn summary(_ctx: &mut dyn varn_types::NativeCtx, _args: &[varn_types::VmValue]) -> Result<varn_types::VmValue, String> {
    print_summary();
    Ok(varn_types::VmValue::null())
}
