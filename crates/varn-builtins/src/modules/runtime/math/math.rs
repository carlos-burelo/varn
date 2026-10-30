use varn_macros::varn_contract;
use varn_types::NativeError;
use varn_types::NativeCtx;

pub struct MathRuntime;

varn_contract! {
    module: "runtime:math",
    contract: "src/modules/runtime/math/math_runtime.vn",
    impl MathRuntime {
        fn abs(_ctx: &mut dyn NativeCtx, x: f64) -> Result<f64, NativeError> { Ok(x.abs()) }
        fn sqrt(_ctx: &mut dyn NativeCtx, x: f64) -> Result<f64, NativeError> { Ok(x.sqrt()) }
        fn sin(_ctx: &mut dyn NativeCtx, x: f64) -> Result<f64, NativeError> { Ok(x.sin()) }
        fn cos(_ctx: &mut dyn NativeCtx, x: f64) -> Result<f64, NativeError> { Ok(x.cos()) }
        fn tan(_ctx: &mut dyn NativeCtx, x: f64) -> Result<f64, NativeError> { Ok(x.tan()) }
        fn floor(_ctx: &mut dyn NativeCtx, x: f64) -> Result<f64, NativeError> { Ok(x.floor()) }
        fn ceil(_ctx: &mut dyn NativeCtx, x: f64) -> Result<f64, NativeError> { Ok(x.ceil()) }
        fn round(_ctx: &mut dyn NativeCtx, x: f64) -> Result<f64, NativeError> { Ok(x.round()) }
        fn random(_ctx: &mut dyn NativeCtx) -> Result<f64, NativeError> {
            Ok(rand::random())
        }
        fn acos(_ctx: &mut dyn NativeCtx, x: f64) -> Result<f64, NativeError> { Ok(x.acos()) }
        fn asin(_ctx: &mut dyn NativeCtx, x: f64) -> Result<f64, NativeError> { Ok(x.asin()) }
        fn atan(_ctx: &mut dyn NativeCtx, x: f64) -> Result<f64, NativeError> { Ok(x.atan()) }
        fn atan2(_ctx: &mut dyn NativeCtx, y: f64, x: f64) -> Result<f64, NativeError> { Ok(y.atan2(x)) }
        fn exp(_ctx: &mut dyn NativeCtx, x: f64) -> Result<f64, NativeError> { Ok(x.exp()) }
        fn log(_ctx: &mut dyn NativeCtx, x: f64) -> Result<f64, NativeError> { Ok(x.ln()) }
        fn log10(_ctx: &mut dyn NativeCtx, x: f64) -> Result<f64, NativeError> { Ok(x.log10()) }
        fn log2(_ctx: &mut dyn NativeCtx, x: f64) -> Result<f64, NativeError> { Ok(x.log2()) }
        fn pow(_ctx: &mut dyn NativeCtx, x: f64, y: f64) -> Result<f64, NativeError> { Ok(x.powf(y)) }
        fn cbrt(_ctx: &mut dyn NativeCtx, x: f64) -> Result<f64, NativeError> { Ok(x.cbrt()) }
        fn hypot(_ctx: &mut dyn NativeCtx, x: f64, y: f64) -> Result<f64, NativeError> { Ok(x.hypot(y)) }
        fn max2(_ctx: &mut dyn NativeCtx, x: f64, y: f64) -> Result<f64, NativeError> { Ok(varn_core::fmax(x, y)) }
        fn min2(_ctx: &mut dyn NativeCtx, x: f64, y: f64) -> Result<f64, NativeError> { Ok(varn_core::fmin(x, y)) }
        fn sign(_ctx: &mut dyn NativeCtx, x: f64) -> Result<f64, NativeError> { Ok(varn_core::fsign(x)) }
    }
}
