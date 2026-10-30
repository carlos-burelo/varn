use varn_macros::varn_contract;
use varn_types::{NativeCtx, NativeError, VmValue, VnArray};

pub struct Bytes;

fn byte_value(value: i64, what: &str) -> Result<u8, NativeError> {
    u8::try_from(value)
        .map_err(|_| NativeError::range_error(format!("{what}: {value} is not a byte (0..=255)")))
}

fn byte_index(ctx: &mut dyn NativeCtx, this: VmValue, index: i64) -> Result<usize, NativeError> {
    let len = ctx.buffer_len(this);
    usize::try_from(index)
        .ok()
        .filter(|i| *i < len)
        .ok_or_else(|| NativeError::range_error(format!("index {index} is out of range for length {len}")))
}

varn_contract! {
    module: "globals",
    class: "Bytes",
    contract: "src/modules/core/types/bytes/bytes.vn",
    impl Bytes {
        fn length(ctx: &mut dyn NativeCtx, this: VmValue) -> i64 {
            ctx.buffer_len(this) as i64
        }

        fn getByte(ctx: &mut dyn NativeCtx, this: VmValue, index: i64) -> Result<i64, NativeError> {
            let at = byte_index(ctx, this, index)?;
            ctx.buffer_get_byte(this, at)
                .map(i64::from)
                .ok_or_else(|| NativeError::type_error("getByte: not a Bytes"))
        }

        fn setByte(ctx: &mut dyn NativeCtx, this: VmValue, index: i64, value: i64) -> Result<(), NativeError> {
            let at = byte_index(ctx, this, index)?;
            let byte = byte_value(value, "setByte")?;
            ctx.buffer_set_byte(this, at, byte);
            Ok(())
        }

        fn slice(ctx: &mut dyn NativeCtx, this: VmValue, start: Option<i64>, end: Option<i64>) -> Result<VmValue, NativeError> {
            let (from, to) = varn_core::slice_bounds(ctx.buffer_len(this), start, end);
            ctx.buffer_slice(this, from, to)
                .ok_or_else(|| NativeError::type_error("slice: not a Bytes"))
        }

        fn toString(ctx: &mut dyn NativeCtx, this: VmValue) -> String {
            ctx.buffer_to_string(this).unwrap_or_default()
        }

        fn toHex(ctx: &mut dyn NativeCtx, this: VmValue) -> String {
            if let Some(bytes) = ctx.buffer_to_bytes(this) {
                let mut out = String::with_capacity(bytes.len() * 2);
                for b in bytes {
                    use std::fmt::Write;
                    let _ = write!(&mut out, "{:02x}", b);
                }
                out
            } else {
                String::new()
            }
        }

        fn toBase64(ctx: &mut dyn NativeCtx, this: VmValue) -> String {
            if let Some(bytes) = ctx.buffer_to_bytes(this) {
                use base64::Engine;
                base64::engine::general_purpose::STANDARD.encode(&bytes)
            } else {
                String::new()
            }
        }

        fn fill(ctx: &mut dyn NativeCtx, this: VmValue, value: i64, start: Option<i64>, end: Option<i64>) -> Result<VmValue, NativeError> {
            let byte = byte_value(value, "fill")?;
            let (from, to) = varn_core::slice_bounds(ctx.buffer_len(this), start, end);
            for idx in from..to {
                ctx.buffer_set_byte(this, idx, byte);
            }
            Ok(this)
        }

        fn copy(ctx: &mut dyn NativeCtx, this: VmValue, target: VmValue, target_start: Option<i64>, source_start: Option<i64>, source_end: Option<i64>) -> i64 {
            let target_len = ctx.buffer_len(target);
            let (from, to) = varn_core::slice_bounds(ctx.buffer_len(this), source_start, source_end);
            let at = varn_core::clamp_index(target_start.unwrap_or(0), target_len);
            let count = (to - from).min(target_len - at);
            for offset in 0..count {
                if let Some(byte) = ctx.buffer_get_byte(this, from + offset) {
                    ctx.buffer_set_byte(target, at + offset, byte);
                }
            }
            count as i64
        }

        fn alloc(ctx: &mut dyn NativeCtx, size: i64) -> Result<VmValue, NativeError> {
            let bytes = usize::try_from(size)
                .ok()
                .filter(|n| *n <= varn_core::MAX_ALLOC_BYTES)
                .ok_or_else(|| {
                    NativeError::range_error(format!(
                        "alloc: size {size} must be within 0..={}",
                        varn_core::MAX_ALLOC_BYTES
                    ))
                })?;
            Ok(ctx.alloc_buffer(bytes))
        }

        fn fromString(ctx: &mut dyn NativeCtx, s: &str) -> VmValue {
            ctx.alloc_buffer_from_bytes(s.as_bytes())
        }

        fn fromBytes(ctx: &mut dyn NativeCtx, bytes: VnArray) -> Result<VmValue, NativeError> {
            let len = bytes.len(ctx);
            let mut raw = Vec::with_capacity(len);
            for i in 0..len {
                let item = bytes
                    .get(ctx, i)
                    .filter(|v| ctx.is_int(*v))
                    .ok_or_else(|| NativeError::type_error(format!("fromBytes: element {i} is not an int")))?;
                let value = ctx.as_int(item);
                raw.push(byte_value(value, &format!("fromBytes: element {i}"))?);
            }
            Ok(ctx.alloc_buffer_from_bytes(&raw))
        }
    }
}
