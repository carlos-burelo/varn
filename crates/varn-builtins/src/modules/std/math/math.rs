use varn_op_macros::varn_contract;
use varn_types::NativeCtx;

pub struct MathRuntime;

varn_contract! {
    module: "runtime:math",
    contract: "src/modules/std/math/runtime/math_runtime.vn",
    impl MathRuntime {
        fn mathAbs(_ctx: &mut dyn NativeCtx, x: f64) -> Result<f64, String> { Ok(x.abs()) }
        fn mathSqrt(_ctx: &mut dyn NativeCtx, x: f64) -> Result<f64, String> { Ok(x.sqrt()) }
        fn mathSin(_ctx: &mut dyn NativeCtx, x: f64) -> Result<f64, String> { Ok(x.sin()) }
        fn mathCos(_ctx: &mut dyn NativeCtx, x: f64) -> Result<f64, String> { Ok(x.cos()) }
        fn mathTan(_ctx: &mut dyn NativeCtx, x: f64) -> Result<f64, String> { Ok(x.tan()) }
        fn mathFloor(_ctx: &mut dyn NativeCtx, x: f64) -> Result<f64, String> { Ok(x.floor()) }
        fn mathCeil(_ctx: &mut dyn NativeCtx, x: f64) -> Result<f64, String> { Ok(x.ceil()) }
        fn mathRound(_ctx: &mut dyn NativeCtx, x: f64) -> Result<f64, String> { Ok(x.round()) }
        fn mathRandom(_ctx: &mut dyn NativeCtx) -> Result<f64, String> {
            use rand::Rng;
            Ok(rand::thread_rng().gen())
        }
        fn mathAcos(_ctx: &mut dyn NativeCtx, x: f64) -> Result<f64, String> { Ok(x.acos()) }
        fn mathAsin(_ctx: &mut dyn NativeCtx, x: f64) -> Result<f64, String> { Ok(x.asin()) }
        fn mathAtan(_ctx: &mut dyn NativeCtx, x: f64) -> Result<f64, String> { Ok(x.atan()) }
        fn mathAtan2(_ctx: &mut dyn NativeCtx, y: f64, x: f64) -> Result<f64, String> { Ok(y.atan2(x)) }
    }
}
