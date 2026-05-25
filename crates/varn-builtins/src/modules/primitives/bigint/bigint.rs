#[allow(unused_imports)]
use std::convert::TryFrom;
#[allow(unused_imports)]
use varn_op_macros::{varn_class, varn_method, varn_module};
use varn_types::{NativeCtx, NativeFnResult, Value, VmValue};

fn get_bigint(ctx: &dyn NativeCtx, this: VmValue) -> Option<i128> {
    match ctx.extract(this) {
        Value::BigInt(n) => Some(*n),
        _ => None,
    }
}

#[varn_module("globals")]
pub(crate) mod dispatch {
    use super::*;

    #[varn_class("bigint")]
    pub mod bigint_class {
        use super::*;

        #[varn_method("toString")]
        pub fn to_string(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            _args: &[VmValue],
        ) -> NativeFnResult {
            if let Some(n) = get_bigint(ctx, this) {
                return Ok(ctx.alloc_str_owned(n.to_string()));
            }
            Ok(ctx.alloc_str("0"))
        }

        #[varn_method("toStr")]
        pub fn to_str(ctx: &mut dyn NativeCtx, this: VmValue, _args: &[VmValue]) -> NativeFnResult {
            if let Some(n) = get_bigint(ctx, this) {
                return Ok(ctx.alloc_str_owned(n.to_string()));
            }
            Ok(ctx.alloc_str("0"))
        }

        #[varn_method("toInt")]
        pub fn to_int(
            _ctx: &mut dyn NativeCtx,
            this: VmValue,
            _args: &[VmValue],
        ) -> NativeFnResult {
            if let Some(n) = get_bigint(_ctx, this) {
                let clamped = i64::try_from(n).unwrap_or_else(|_| {
                    if n.is_negative() {
                        i64::MIN
                    } else {
                        i64::MAX
                    }
                });
                return Ok(VmValue::from_int(clamped));
            }
            Ok(VmValue::from_int(0))
        }

        #[varn_method("toFloat")]
        pub fn to_float(
            _ctx: &mut dyn NativeCtx,
            this: VmValue,
            _args: &[VmValue],
        ) -> NativeFnResult {
            if let Some(n) = get_bigint(_ctx, this) {
                return Ok(VmValue::from_f64(n as f64));
            }
            Ok(VmValue::from_f64(0.0))
        }
    }
}
