use varn_op_macros::varn_contract;
use varn_types::NativeCtx;

pub struct MathRuntime;

varn_contract! {
    module: "runtime:math",
    contract: "src/modules/host/math/math_runtime.vn",
    impl MathRuntime {
        fn abs(_ctx: &mut dyn NativeCtx, x: f64) -> Result<f64, String> { Ok(x.abs()) }
        fn sqrt(_ctx: &mut dyn NativeCtx, x: f64) -> Result<f64, String> { Ok(x.sqrt()) }
        fn sin(_ctx: &mut dyn NativeCtx, x: f64) -> Result<f64, String> { Ok(x.sin()) }
        fn cos(_ctx: &mut dyn NativeCtx, x: f64) -> Result<f64, String> { Ok(x.cos()) }
        fn tan(_ctx: &mut dyn NativeCtx, x: f64) -> Result<f64, String> { Ok(x.tan()) }
        fn floor(_ctx: &mut dyn NativeCtx, x: f64) -> Result<f64, String> { Ok(x.floor()) }
        fn ceil(_ctx: &mut dyn NativeCtx, x: f64) -> Result<f64, String> { Ok(x.ceil()) }
        fn round(_ctx: &mut dyn NativeCtx, x: f64) -> Result<f64, String> { Ok(x.round()) }
        fn random(_ctx: &mut dyn NativeCtx) -> Result<f64, String> {
            use rand::Rng;
            Ok(rand::thread_rng().gen())
        }
        fn acos(_ctx: &mut dyn NativeCtx, x: f64) -> Result<f64, String> { Ok(x.acos()) }
        fn asin(_ctx: &mut dyn NativeCtx, x: f64) -> Result<f64, String> { Ok(x.asin()) }
        fn atan(_ctx: &mut dyn NativeCtx, x: f64) -> Result<f64, String> { Ok(x.atan()) }
        fn atan2(_ctx: &mut dyn NativeCtx, y: f64, x: f64) -> Result<f64, String> { Ok(y.atan2(x)) }
    }
}
