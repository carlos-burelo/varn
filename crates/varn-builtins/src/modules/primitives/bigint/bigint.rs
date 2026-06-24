use varn_op_macros::varn_contract;
use varn_types::{NativeCtx, Value, VmValue};

pub struct BigInt;

fn get_bigint(ctx: &dyn NativeCtx, this: VmValue) -> Option<i128> {
    match ctx.extract(this) {
        Value::BigInt(n) => Some(*n),
        _ => None,
    }
}

varn_contract! {
    module: "globals",
    class: "bigint",
    contract: "src/modules/primitives/bigint/bigint.vn",
    impl BigInt {
        fn toString(ctx: &mut dyn NativeCtx, this: VmValue) -> String {
            get_bigint(ctx, this).map(|n| n.to_string()).unwrap_or_else(|| "0".to_string())
        }
        fn toStr(ctx: &mut dyn NativeCtx, this: VmValue) -> String {
            get_bigint(ctx, this).map(|n| n.to_string()).unwrap_or_else(|| "0".to_string())
        }
        fn toInt(ctx: &mut dyn NativeCtx, this: VmValue) -> i64 {
            match get_bigint(ctx, this) {
                Some(n) => i64::try_from(n).unwrap_or(if n.is_negative() { i64::MIN } else { i64::MAX }),
                None => 0,
            }
        }
        fn toFloat(ctx: &mut dyn NativeCtx, this: VmValue) -> f64 {
            get_bigint(ctx, this).map(|n| n as f64).unwrap_or(0.0)
        }
    }
}
