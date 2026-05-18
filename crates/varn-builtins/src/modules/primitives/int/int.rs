#[allow(unused_imports)]
use varn_op_macros::{varn_class, varn_method, varn_module, varn_static};
use varn_types::{NativeCtx, NativeFnResult, VmValue};

#[varn_module("globals")]
pub(crate) mod dispatch {
    use super::*;

    #[varn_class("int")]
    pub mod int_class {
        use super::*;

        #[varn_static("parse")]
        pub fn parse(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
            if let Some(&v) = args.first() {
                if let Some(s) = ctx.str_owned(v) {
                    return Ok(VmValue::from_int(s.trim().parse::<i64>().unwrap_or(0)));
                }
            }
            Ok(VmValue::from_int(0))
        }

        #[varn_static("isInteger")]
        pub fn is_integer(_ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
            if let Some(&v) = args.first() {
                return Ok(VmValue::from_bool(v.is_int()));
            }
            Ok(VmValue::bool_false())
        }

        #[varn_method("toString")]
        pub fn to_str(ctx: &mut dyn NativeCtx, this: VmValue, _args: &[VmValue]) -> NativeFnResult {
            if this.is_int() {
                return Ok(ctx.alloc_str_owned(this.as_int().to_string()));
            }
            Ok(ctx.alloc_str("0"))
        }

        #[varn_method("abs")]
        pub fn abs(_ctx: &mut dyn NativeCtx, this: VmValue, _args: &[VmValue]) -> NativeFnResult {
            if this.is_int() {
                return Ok(VmValue::from_int(this.as_int().abs()));
            }
            Ok(VmValue::from_int(0))
        }
    }
}

pub fn int_is_integer(_ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
    if let Some(&v) = args.first() {
        return Ok(VmValue::from_bool(v.is_int()));
    }
    Ok(VmValue::bool_false())
}

pub fn int_parse(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
    if let Some(&v) = args.first() {
        if let Some(s) = ctx.str_owned(v) {
            return Ok(VmValue::from_int(s.trim().parse::<i64>().unwrap_or(0)));
        }
    }
    Ok(VmValue::from_int(0))
}
