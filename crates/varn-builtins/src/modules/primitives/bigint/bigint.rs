use num_traits::ToPrimitive;
use varn_op_macros::varn_contract;
use varn_types::{NativeCtx, NativeError, Value, VmValue};

pub struct BigInt;

fn get_bigint(ctx: &dyn NativeCtx, this: VmValue) -> Option<num_bigint::BigInt> {
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
        fn toInt(ctx: &mut dyn NativeCtx, this: VmValue) -> Result<i64, NativeError> {
            let n = get_bigint(ctx, this).ok_or_else(|| NativeError::from("bigint.toInt: not a bigint"))?;
            n.to_i64().ok_or_else(|| {
                NativeError::integer_overflow(format!("integer overflow: {n} does not fit int"))
            })
        }
        fn toFloat(ctx: &mut dyn NativeCtx, this: VmValue) -> f64 {
            get_bigint(ctx, this).and_then(|n| n.to_f64()).unwrap_or(0.0)
        }
    }
}
