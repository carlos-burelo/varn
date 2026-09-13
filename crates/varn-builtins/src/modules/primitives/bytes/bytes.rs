use varn_op_macros::varn_contract;
use varn_types::{NativeCtx, VmValue, VnArray};

pub struct Bytes;

varn_contract! {
    module: "globals",
    class: "Bytes",
    contract: "src/modules/primitives/bytes/bytes.vn",
    impl Bytes {
        fn length(ctx: &mut dyn NativeCtx, this: VmValue) -> i64 {
            ctx.buffer_len(this) as i64
        }

        fn getByte(ctx: &mut dyn NativeCtx, this: VmValue, index: i64) -> i64 {
            ctx.buffer_get_byte(this, index as usize).unwrap_or(0) as i64
        }

        fn setByte(ctx: &mut dyn NativeCtx, this: VmValue, index: i64, value: i64) {
            ctx.buffer_set_byte(this, index as usize, value as u8);
        }

        fn slice(ctx: &mut dyn NativeCtx, this: VmValue, start: Option<i64>, end: Option<i64>) -> VmValue {
            let len = ctx.buffer_len(this);
            let s = start.unwrap_or(0).max(0) as usize;
            let e = end.unwrap_or(len as i64).max(0) as usize;
            ctx.buffer_slice(this, s, e).unwrap_or_else(VmValue::null)
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

        fn fill(ctx: &mut dyn NativeCtx, this: VmValue, value: i64, start: Option<i64>, end: Option<i64>) -> VmValue {
            let len = ctx.buffer_len(this);
            let s = start.unwrap_or(0).max(0) as usize;
            let e = end.unwrap_or(len as i64).min(len as i64) as usize;
            for idx in s..e {
                ctx.buffer_set_byte(this, idx, value as u8);
            }
            this
        }

        fn copy(ctx: &mut dyn NativeCtx, this: VmValue, target: VmValue, target_start: Option<i64>, source_start: Option<i64>, source_end: Option<i64>) -> i64 {
            let s_len = ctx.buffer_len(this);
            let t_len = ctx.buffer_len(target);
            let mut s = source_start.unwrap_or(0).max(0) as usize;
            let mut t = target_start.unwrap_or(0).max(0) as usize;
            let end = source_end.unwrap_or(s_len as i64).min(s_len as i64) as usize;
            let mut copied = 0;
            while s < end && t < t_len {
                if let Some(byte) = ctx.buffer_get_byte(this, s) {
                    ctx.buffer_set_byte(target, t, byte);
                    copied += 1;
                }
                s += 1;
                t += 1;
            }
            copied
        }

        fn alloc(ctx: &mut dyn NativeCtx, size: i64) -> VmValue {
            ctx.alloc_buffer(size.max(0) as usize)
        }

        fn fromString(ctx: &mut dyn NativeCtx, s: &str) -> VmValue {
            ctx.alloc_buffer_from_bytes(s.as_bytes())
        }

        fn fromBytes(ctx: &mut dyn NativeCtx, bytes: VnArray) -> VmValue {
            let len = bytes.len(ctx);
            let mut raw = Vec::with_capacity(len);
            for i in 0..len {
                if let Some(v) = bytes.get(ctx, i) {
                    raw.push(ctx.as_int(v) as u8);
                }
            }
            ctx.alloc_buffer_from_bytes(&raw)
        }
    }
}
