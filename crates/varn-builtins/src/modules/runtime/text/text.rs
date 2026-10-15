use varn_macros::varn_contract;
use varn_types::{NativeCtx, VmValue};

pub struct TextRuntime;

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
            let ids_v = ctx.alloc_array(ids);
            let adv_v = ctx.alloc_array(adv);
            let clusters_v = ctx.alloc_array(clusters);
            let width_v = VmValue::from_f64(run.width as f64);
            let count_v = VmValue::from_int(run.glyphs.len() as i64);
            let o = ctx.alloc_object();
            ctx.set_field(o, "glyphs", ids_v);
            ctx.set_field(o, "advances", adv_v);
            ctx.set_field(o, "clusters", clusters_v);
            ctx.set_field(o, "width", width_v);
            ctx.set_field(o, "count", count_v);
            Ok(o)
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
            let bitmap_v = ctx.alloc_buffer_from_bytes(&g.bitmap);
            let w_v = VmValue::from_int(g.w as i64);
            let h_v = VmValue::from_int(g.h as i64);
            let bx_v = VmValue::from_int(g.bx as i64);
            let by_v = VmValue::from_int(g.by as i64);
            let adv_v = VmValue::from_f64(g.advance as f64);
            let o = ctx.alloc_object();
            ctx.set_field(o, "w", w_v);
            ctx.set_field(o, "h", h_v);
            ctx.set_field(o, "bx", bx_v);
            ctx.set_field(o, "by", by_v);
            ctx.set_field(o, "advance", adv_v);
            ctx.set_field(o, "bitmap", bitmap_v);
            Ok(o)
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
