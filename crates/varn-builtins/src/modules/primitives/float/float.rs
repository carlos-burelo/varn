#[allow(unused_imports)]
use varn_op_macros::{varn_class, varn_method, varn_module, varn_static};
use varn_types::{NativeCtx, NativeFnResult, VmValue};

#[varn_module("globals")]
pub(crate) mod dispatch {
    use super::*;

    #[varn_class("float")]
    pub mod float_class {
        use super::*;

        #[varn_method("toString")]
        pub fn to_str(ctx: &mut dyn NativeCtx, this: VmValue, _args: &[VmValue]) -> NativeFnResult {
            if this.is_f64() {
                Ok(ctx.alloc_str_owned(this.as_f64().to_string()))
            } else if this.is_int() {
                Ok(ctx.alloc_str_owned(this.as_int().to_string()))
            } else {
                Ok(ctx.alloc_str(""))
            }
        }

        #[varn_static("isNaN")]
        pub fn is_nan(_ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
            if let Some(&v) = args.first() {
                return Ok(VmValue::from_bool(v.is_f64() && v.as_f64().is_nan()));
            }
            Ok(VmValue::bool_false())
        }

        #[varn_static("isFinite")]
        pub fn is_finite(_ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
            if let Some(&v) = args.first() {
                if v.is_f64() {
                    return Ok(VmValue::from_bool(v.as_f64().is_finite()));
                }
                return Ok(VmValue::from_bool(v.is_int()));
            }
            Ok(VmValue::bool_false())
        }

        #[varn_static("parse")]
        pub fn parse(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
            let s = args.first().map(|&v| ctx.str_repr(v)).unwrap_or_default();
            let f = s.trim().parse::<f64>().unwrap_or(f64::NAN);
            Ok(VmValue::from_f64(f))
        }
    }
}

pub fn float_is_finite(_ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
    if let Some(&v) = args.first() {
        if v.is_f64() {
            return Ok(VmValue::from_bool(v.as_f64().is_finite()));
        }
        return Ok(VmValue::from_bool(v.is_int()));
    }
    Ok(VmValue::bool_false())
}

pub fn float_is_nan(_ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
    if let Some(&v) = args.first() {
        return Ok(VmValue::from_bool(v.is_f64() && v.as_f64().is_nan()));
    }
    Ok(VmValue::bool_false())
}

pub fn float_parse(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
    let s = args.first().map(|&v| ctx.str_repr(v)).unwrap_or_default();
    let f = s.trim().parse::<f64>().unwrap_or(f64::NAN);
    Ok(VmValue::from_f64(f))
}
