use varn_op_macros::varn_contract;
use varn_types::marshal::VnArray;
use varn_types::{NativeCtx, VmValue};

pub struct BufferRuntime;

varn_contract! {
    module: "runtime:buffer",
    contract: "src/modules/host/buffer/buffer_runtime.vn",
    impl BufferRuntime {
        fn alloc(ctx: &mut dyn NativeCtx, size: i64) -> Result<VmValue, String> {
            if size < 0 {
                return Err("buffer.alloc: size must be non-negative".to_string());
            }
            Ok(ctx.alloc_buffer(size as usize))
        }

        fn fromBytes(ctx: &mut dyn NativeCtx, bytes: VnArray) -> Result<VmValue, String> {
            let len = bytes.len(ctx);
            let mut raw = Vec::with_capacity(len);
            for i in 0..len {
                if let Some(v) = bytes.get(ctx, i) {
                    raw.push(ctx.as_int(v) as u8);
                }
            }
            Ok(ctx.alloc_buffer_from_bytes(&raw))
        }

        fn fromString(ctx: &mut dyn NativeCtx, s: &str) -> Result<VmValue, String> {
            Ok(ctx.alloc_buffer_from_bytes(s.as_bytes()))
        }

        fn len(ctx: &mut dyn NativeCtx, buf: VmValue) -> Result<i64, String> {
            if !ctx.is_buffer(buf) {
                return Err("buffer.len: argument is not a Buffer".to_string());
            }
            Ok(ctx.buffer_len(buf) as i64)
        }

        fn getByte(ctx: &mut dyn NativeCtx, buf: VmValue, index: i64) -> Result<i64, String> {
            if !ctx.is_buffer(buf) {
                return Err("buffer.getByte: argument is not a Buffer".to_string());
            }
            let byte = ctx.buffer_get_byte(buf, index as usize)
                .ok_or_else(|| format!("buffer.getByte: index out of bounds ({index})"))?;
            Ok(byte as i64)
        }

        fn setByte(ctx: &mut dyn NativeCtx, buf: VmValue, index: i64, value: i64) -> Result<(), String> {
            if !ctx.is_buffer(buf) {
                return Err("buffer.setByte: argument is not a Buffer".to_string());
            }
            if !ctx.buffer_set_byte(buf, index as usize, value as u8) {
                return Err(format!("buffer.setByte: index out of bounds ({index})"));
            }
            Ok(())
        }

        fn slice(ctx: &mut dyn NativeCtx, buf: VmValue, start: i64, end: i64) -> Result<VmValue, String> {
            if !ctx.is_buffer(buf) {
                return Err("buffer.slice: argument is not a Buffer".to_string());
            }
            let sub = ctx.buffer_slice(buf, start as usize, end as usize)
                .ok_or_else(|| "buffer.slice failed".to_string())?;
            Ok(sub)
        }

        fn toString(ctx: &mut dyn NativeCtx, buf: VmValue) -> Result<String, String> {
            if !ctx.is_buffer(buf) {
                return Err("buffer.toString: argument is not a Buffer".to_string());
            }
            ctx.buffer_to_string(buf).ok_or_else(|| "buffer.toString: invalid UTF-8".to_string())
        }
    }
}
