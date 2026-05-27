use varn_op_macros::{varn_module};
use varn_types::{NativeCtx, VmValue};

#[varn_module("runtime:math")]
pub(crate) mod dispatch {
    use super::*;

    #[varn_fn("mathAbs")]
    pub fn abs(_ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let v = args.first().copied().unwrap_or(VmValue::null());
        if v.is_int() {
            Ok(VmValue::from_int(v.as_int().abs()))
        } else if v.is_f64() {
            Ok(VmValue::from_f64(v.as_f64().abs()))
        } else {
            Ok(v)
        }
    }

    #[varn_fn("mathSqrt")]
    pub fn sqrt(_ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let v = args.first().copied().unwrap_or(VmValue::null());
        let val = if v.is_int() {
            v.as_int() as f64
        } else if v.is_f64() {
            v.as_f64()
        } else {
            0.0
        };
        Ok(VmValue::from_f64(val.sqrt()))
    }

    #[varn_fn("mathSin")]
    pub fn sin(_ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let v = args.first().copied().unwrap_or(VmValue::null());
        let val = if v.is_int() {
            v.as_int() as f64
        } else if v.is_f64() {
            v.as_f64()
        } else {
            0.0
        };
        Ok(VmValue::from_f64(val.sin()))
    }

    #[varn_fn("mathCos")]
    pub fn cos(_ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let v = args.first().copied().unwrap_or(VmValue::null());
        let val = if v.is_int() {
            v.as_int() as f64
        } else if v.is_f64() {
            v.as_f64()
        } else {
            0.0
        };
        Ok(VmValue::from_f64(val.cos()))
    }

    #[varn_fn("mathTan")]
    pub fn tan(_ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let v = args.first().copied().unwrap_or(VmValue::null());
        let val = if v.is_int() {
            v.as_int() as f64
        } else if v.is_f64() {
            v.as_f64()
        } else {
            0.0
        };
        Ok(VmValue::from_f64(val.tan()))
    }

    #[varn_fn("mathFloor")]
    pub fn floor(_ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let v = args.first().copied().unwrap_or(VmValue::null());
        if v.is_int() {
            return Ok(v);
        }
        let val = if v.is_f64() { v.as_f64() } else { 0.0 };
        Ok(VmValue::from_f64(val.floor()))
    }

    #[varn_fn("mathCeil")]
    pub fn ceil(_ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let v = args.first().copied().unwrap_or(VmValue::null());
        if v.is_int() {
            return Ok(v);
        }
        let val = if v.is_f64() { v.as_f64() } else { 0.0 };
        Ok(VmValue::from_f64(val.ceil()))
    }

    #[varn_fn("mathRound")]
    pub fn round(_ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let v = args.first().copied().unwrap_or(VmValue::null());
        if v.is_int() {
            return Ok(v);
        }
        let val = if v.is_f64() { v.as_f64() } else { 0.0 };
        Ok(VmValue::from_f64(val.round()))
    }

    #[varn_fn("mathRandom")]
    pub fn random(_ctx: &mut dyn NativeCtx, _args: &[VmValue]) -> Result<VmValue, String> {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        Ok(VmValue::from_f64(rng.gen()))
    }
}
