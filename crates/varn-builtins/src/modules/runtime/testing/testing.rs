use std::sync::atomic::{AtomicU64, Ordering};
use varn_macros::varn_contract;
use varn_types::NativeError;
use varn_types::{NativeCtx, VmValue};

pub struct TestingRuntime;


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

fn numeric(ctx: &dyn NativeCtx, v: VmValue) -> Option<f64> {
    (v.is_int() || v.is_f64()).then(|| ctx.to_f64(v))
}

fn deep_equal(ctx: &dyn NativeCtx, a: VmValue, b: VmValue) -> bool {
    if a == b {
        return true;
    }
    if ctx.is_string(a) || ctx.is_string(b) {
        return ctx.is_string(a) && ctx.is_string(b) && ctx.str_owned(a) == ctx.str_owned(b);
    }
    if let (Some(x), Some(y)) = (numeric(ctx, a), numeric(ctx, b)) {
        return x == y;
    }
    if ctx.is_array(a) || ctx.is_array(b) {
        if !(ctx.is_array(a) && ctx.is_array(b))
            || ctx.is_tuple(a) != ctx.is_tuple(b)
            || ctx.array_len(a) != ctx.array_len(b)
        {
            return false;
        }
        return (0..ctx.array_len(a)).all(|i| {
            let x = ctx.array_get(a, i).unwrap_or_else(VmValue::null);
            let y = ctx.array_get(b, i).unwrap_or_else(VmValue::null);
            deep_equal(ctx, x, y)
        });
    }
    if let (Some(x), Some(y)) = (ctx.as_map(a), ctx.as_map(b)) {
        let (x, y) = (x.read(), y.read());
        return x.len() == y.len()
            && x.iter().all(|(k, v)| {
                y.iter()
                    .any(|(k2, v2)| deep_equal(ctx, k.0, k2.0) && deep_equal(ctx, *v, *v2))
            });
    }
    if let (Some(x), Some(y)) = (ctx.as_set(a), ctx.as_set(b)) {
        let (x, y) = (x.read(), y.read());
        return x.len() == y.len()
            && x.iter().all(|k| y.iter().any(|k2| deep_equal(ctx, k.0, k2.0)));
    }
    if ctx.is_object(a) && ctx.is_object(b) {
        let mut left = Vec::new();
        ctx.object_for_each(a, &mut |name, v| left.push((name.to_owned(), v)));
        let mut right = Vec::new();
        ctx.object_for_each(b, &mut |name, v| right.push((name.to_owned(), v)));
        return left.len() == right.len()
            && left.iter().all(|(name, v)| {
                right
                    .iter()
                    .find(|(other, _)| other == name)
                    .is_some_and(|(_, w)| deep_equal(ctx, *v, *w))
            });
    }
    false
}

varn_contract! {
    module: "runtime:testing",
    contract: "src/modules/runtime/testing/testing_runtime.vn",
    impl TestingRuntime {
        fn deepEq(ctx: &mut dyn NativeCtx, a: VmValue, b: VmValue) -> Result<bool, NativeError> {
            Ok(deep_equal(ctx, a, b))
        }
    }
}
