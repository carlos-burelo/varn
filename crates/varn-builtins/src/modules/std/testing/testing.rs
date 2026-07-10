use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use varn_op_macros::varn_contract;
use varn_types::{NativeCtx, VmValue};

pub struct TestRuntime;

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

fn print_summary() {
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

pub fn summary(_ctx: &mut dyn NativeCtx, _args: &[VmValue]) -> Result<VmValue, String> {
    print_summary();
    Ok(VmValue::null())
}

varn_contract! {
    module: "runtime:test",
    contract: "src/modules/host/testing/test_runtime.vn",
    impl TestRuntime {
        fn testAssert(_ctx: &mut dyn NativeCtx, label: &str, condition: bool) -> Result<(), String> {
            if condition {
                PASSED.fetch_add(1, Ordering::Relaxed);
            } else {
                FAILED.fetch_add(1, Ordering::Relaxed);
                if !SILENT.load(Ordering::Relaxed) {
                    println!("FAIL: {label}");
                }
            }
            Ok(())
        }
        fn testAssertEqual(ctx: &mut dyn NativeCtx, actual: VmValue, expected: VmValue, message: Option<&str>) -> Result<(), String> {
            if actual == expected {
                PASSED.fetch_add(1, Ordering::Relaxed);
            } else {
                FAILED.fetch_add(1, Ordering::Relaxed);
                if !SILENT.load(Ordering::Relaxed) {
                    let msg = message.map(|m| m.to_string()).unwrap_or_else(|| {
                        format!("expected {} but got {}", ctx.str_repr(expected), ctx.str_repr(actual))
                    });
                    println!("FAIL: {msg}");
                }
            }
            Ok(())
        }
        fn testSummary(_ctx: &mut dyn NativeCtx) -> Result<(), String> {
            print_summary();
            Ok(())
        }
    }
}
