use std::sync::atomic::AtomicBool;
use std::sync::atomic::{AtomicU64, Ordering};
use varn_op_macros::varn_module;
use varn_types::{NativeCtx, VmValue};

#[varn_module("runtime:test")]
pub(crate) mod dispatch {
    use super::*;

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

    #[varn_fn("testAssert")]
    pub fn assert(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let cond = args.get(1).map(|&v| v.as_bool()).unwrap_or(false);
        if cond {
            PASSED.fetch_add(1, Ordering::Relaxed);
        } else {
            let label = args
                .first()
                .map(|&v| ctx.str_repr(v))
                .unwrap_or_else(|| "?".into());
            FAILED.fetch_add(1, Ordering::Relaxed);
            if !SILENT.load(Ordering::Relaxed) {
                println!("FAIL: {label}");
            }
        }
        Ok(VmValue::null())
    }

    #[varn_fn("testAssertEqual")]
    pub fn assert_equal(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let actual = args.first().copied().unwrap_or(VmValue::null());
        let expected = args.get(1).copied().unwrap_or(VmValue::null());
        if actual == expected {
            PASSED.fetch_add(1, Ordering::Relaxed);
        } else {
            FAILED.fetch_add(1, Ordering::Relaxed);
            let msg = format!(
                "expected {} but got {}",
                ctx.str_repr(expected),
                ctx.str_repr(actual)
            );
            if !SILENT.load(Ordering::Relaxed) {
                println!("FAIL: {msg}");
            }
            return Err(msg);
        }
        Ok(VmValue::null())
    }

    #[varn_fn("testSummary")]
    pub fn summary(_ctx: &mut dyn NativeCtx, _args: &[VmValue]) -> Result<VmValue, String> {
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
        Ok(VmValue::null())
    }
}

pub use dispatch::{inc_failed, inc_passed, reset_testing_counters, set_testing_silent};
