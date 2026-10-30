use varn_core::StdErrorClass;
use varn_macros::varn_contract;
use varn_types::NativeError;
use varn_types::{NativeCtx, VmValue};

pub struct TextRuntime;

varn_macros::varn_record! {
    contract: "src/modules/runtime/text/text_runtime.vn",
    interface: "ShapedRun",
    struct ShapedRun;
}

varn_contract! {
    module: "runtime:text",
    contract: "src/modules/runtime/text/text_runtime.vn",
    impl TextRuntime {
        fn fontCount(_ctx: &mut dyn NativeCtx) -> Result<i64, NativeError> {
            Ok(varn_text::fonts().len() as i64)
        }
        fn fonts(ctx: &mut dyn NativeCtx) -> Result<Vec<VmValue>, NativeError> {
            Ok(varn_text::fonts()
                .into_iter()
                .map(|s| ctx.alloc_str_owned(s))
                .collect())
        }
        fn shape(ctx: &mut dyn NativeCtx, text: &str, size: f64) -> Result<VmValue, NativeError> {
            let run = varn_text::shape_run(text, size as f32).map_err(|e| NativeError::std(StdErrorClass::Parse, e))?;
            let chars: Vec<VmValue> = run
                .glyphs
                .iter()
                .map(|g| VmValue::from_int(g.ch as i64))
                .collect();
            let adv: Vec<VmValue> = run
                .glyphs
                .iter()
                .map(|g| VmValue::from_f64(g.advance as f64))
                .collect();
            let clusters: Vec<VmValue> = run
                .glyphs
                .iter()
                .map(|g| VmValue::from_int(g.cluster as i64))
                .collect();
            let glyphs = ctx.alloc_array(chars);
            let advances = ctx.alloc_array(adv);
            let clusters = ctx.alloc_array(clusters);
            Ok(ShapedRun {
                glyphs,
                advances,
                clusters,
                width: run.width as f64,
                count: run.glyphs.len() as i64,
            }
            .alloc(ctx))
        }
        fn ascent(_ctx: &mut dyn NativeCtx, size: f64) -> Result<f64, NativeError> {
            Ok(varn_text::font_ascent(size as f32).map_err(|e| NativeError::std(StdErrorClass::Parse, e))? as f64)
        }
        fn descent(_ctx: &mut dyn NativeCtx, size: f64) -> Result<f64, NativeError> {
            Ok(varn_text::font_descent(size as f32).map_err(|e| NativeError::std(StdErrorClass::Parse, e))? as f64)
        }
        fn wrap(ctx: &mut dyn NativeCtx, text: &str, size: f64, max: f64) -> Result<Vec<VmValue>, NativeError> {
            Ok(varn_text::wrap_lines(text, size as f32, max as f32).map_err(|e| NativeError::std(StdErrorClass::Parse, e))?
                .into_iter()
                .map(|s| ctx.alloc_str_owned(s))
                .collect())
        }
    }
}
