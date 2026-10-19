use varn_macros::varn_contract;
use varn_types::{NativeCtx, VmValue};

pub struct TextRuntime;

varn_macros::varn_record! {
    contract: "src/modules/runtime/text/text_runtime.vn",
    interface: "ShapedRun",
    struct ShapedRun;
}

varn_macros::varn_record! {
    contract: "src/modules/runtime/text/text_runtime.vn",
    interface: "RasterGlyph",
    struct RasterGlyph;
}

varn_contract! {
    module: "runtime:text",
    contract: "src/modules/runtime/text/text_runtime.vn",
    impl TextRuntime {
        fn fontCount(_ctx: &mut dyn NativeCtx) -> Result<i64, String> {
            Ok(varn_text::fonts().len() as i64)
        }
        fn fonts(ctx: &mut dyn NativeCtx) -> Result<Vec<VmValue>, String> {
            Ok(varn_text::fonts()
                .into_iter()
                .map(|s| ctx.alloc_str_owned(s))
                .collect())
        }
        fn shape(ctx: &mut dyn NativeCtx, text: &str, size: f64) -> Result<VmValue, String> {            let run = varn_text::shape_run(text, size as f32)?;
            let ids: Vec<VmValue> = run
                .glyphs
                .iter()
                .map(|g| VmValue::from_int(g.id as i64))
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
            let glyphs = ctx.alloc_array(ids);
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
        fn rasterGlyph(ctx: &mut dyn NativeCtx, ch: &str, size: f64) -> Result<VmValue, String> {
            let c = ch.chars().next().ok_or_else(|| "text.raster: empty string".to_string())?;
            let mut scratch = [0u8; 4];
            let run = varn_text::shape_run(c.encode_utf8(&mut scratch), size as f32)?;
            let first = run
                .glyphs
                .first()
                .ok_or_else(|| "text.raster: no glyph".to_string())?;
            let g = varn_text::raster_glyph(first.id, first.advance, size as f32)?;
            let bitmap = ctx.alloc_buffer_from_bytes(&g.bitmap);
            Ok(RasterGlyph {
                w: g.w as i64,
                h: g.h as i64,
                bx: g.bx as i64,
                by: g.by as i64,
                advance: g.advance as f64,
                bitmap,
            }
            .alloc(ctx))
        }
        fn glyphCacheSize(_ctx: &mut dyn NativeCtx) -> Result<i64, String> {
            Ok(varn_text::cache_size() as i64)
        }
        fn ascent(_ctx: &mut dyn NativeCtx, size: f64) -> Result<f64, String> {
            Ok(varn_text::font_ascent(size as f32)? as f64)
        }
        fn descent(_ctx: &mut dyn NativeCtx, size: f64) -> Result<f64, String> {
            Ok(varn_text::font_descent(size as f32)? as f64)
        }
        fn wrap(ctx: &mut dyn NativeCtx, text: &str, size: f64, max: f64) -> Result<Vec<VmValue>, String> {
            Ok(varn_text::wrap_lines(text, size as f32, max as f32)?
                .into_iter()
                .map(|s| ctx.alloc_str_owned(s))
                .collect())
        }
    }
}
