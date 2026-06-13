use varn_op_macros::varn_contract;
use varn_types::NativeCtx;

/// Native implementation backing the `runtime:math` contract
/// (`src/modules/std/math/runtime/math_runtime.vn`).
pub struct MathRuntime;

varn_contract! {
    module: "runtime:math",
    contract: "src/modules/std/math/runtime/math_runtime.vn",
    impl MathRuntime {
        fn mathAbs(_ctx: &mut dyn NativeCtx, x: f64) -> f64 { x.abs() }
        fn mathSqrt(_ctx: &mut dyn NativeCtx, x: f64) -> f64 { x.sqrt() }
        fn mathSin(_ctx: &mut dyn NativeCtx, x: f64) -> f64 { x.sin() }
        fn mathCos(_ctx: &mut dyn NativeCtx, x: f64) -> f64 { x.cos() }
        fn mathTan(_ctx: &mut dyn NativeCtx, x: f64) -> f64 { x.tan() }
        fn mathFloor(_ctx: &mut dyn NativeCtx, x: f64) -> f64 { x.floor() }
        fn mathCeil(_ctx: &mut dyn NativeCtx, x: f64) -> f64 { x.ceil() }
        fn mathRound(_ctx: &mut dyn NativeCtx, x: f64) -> f64 { x.round() }
        fn mathRandom(_ctx: &mut dyn NativeCtx) -> f64 {
            use rand::Rng;
            rand::thread_rng().gen()
        }
    }
}
