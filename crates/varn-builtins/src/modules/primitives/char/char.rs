#[allow(unused_imports)]
use varn_op_macros::{varn_class, varn_method, varn_module, varn_static};
use varn_types::{NativeCtx, NativeFnResult, Value, VmValue};

fn get_char(ctx: &dyn NativeCtx, this: VmValue) -> Option<char> {
    if let Value::Char(c) = ctx.extract(this) {
        return Some(c);
    }
    if let Value::Str(s) = ctx.extract(this) {
        return s.chars().next();
    }
    None
}

#[varn_module("globals")]
pub(crate) mod dispatch {
    use super::*;

    #[varn_class("char")]
    pub mod char_class {
        use super::*;

        #[varn_method("toString")]
        pub fn to_str(ctx: &mut dyn NativeCtx, this: VmValue, _args: &[VmValue]) -> NativeFnResult {
            if let Some(c) = get_char(ctx, this) {
                return Ok(ctx.alloc_str_owned(c.to_string()));
            }
            Ok(ctx.alloc_str(""))
        }

        #[varn_method("charCodeAt")]
        pub fn char_code_at(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            _args: &[VmValue],
        ) -> NativeFnResult {
            if let Some(c) = get_char(ctx, this) {
                return Ok(VmValue::from_int(c as i64));
            }
            Ok(VmValue::from_int(0))
        }

        #[varn_method("isAlphabetic")]
        pub fn is_alphabetic(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            _args: &[VmValue],
        ) -> NativeFnResult {
            Ok(VmValue::from_bool(
                get_char(ctx, this)
                    .map(|c| c.is_alphabetic())
                    .unwrap_or(false),
            ))
        }

        #[varn_method("isAlphanumeric")]
        pub fn is_alphanumeric(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            _args: &[VmValue],
        ) -> NativeFnResult {
            Ok(VmValue::from_bool(
                get_char(ctx, this)
                    .map(|c| c.is_alphanumeric())
                    .unwrap_or(false),
            ))
        }

        #[varn_method("isDigit")]
        pub fn is_digit(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            _args: &[VmValue],
        ) -> NativeFnResult {
            Ok(VmValue::from_bool(
                get_char(ctx, this)
                    .map(|c| c.is_ascii_digit())
                    .unwrap_or(false),
            ))
        }

        #[varn_method("isWhitespace")]
        pub fn is_whitespace(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            _args: &[VmValue],
        ) -> NativeFnResult {
            Ok(VmValue::from_bool(
                get_char(ctx, this)
                    .map(|c| c.is_whitespace())
                    .unwrap_or(false),
            ))
        }

        #[varn_method("isUppercase")]
        pub fn is_uppercase(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            _args: &[VmValue],
        ) -> NativeFnResult {
            Ok(VmValue::from_bool(
                get_char(ctx, this)
                    .map(|c| c.is_uppercase())
                    .unwrap_or(false),
            ))
        }

        #[varn_method("isLowercase")]
        pub fn is_lowercase(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            _args: &[VmValue],
        ) -> NativeFnResult {
            Ok(VmValue::from_bool(
                get_char(ctx, this)
                    .map(|c| c.is_lowercase())
                    .unwrap_or(false),
            ))
        }

        #[varn_method("isAscii")]
        pub fn is_ascii(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            _args: &[VmValue],
        ) -> NativeFnResult {
            Ok(VmValue::from_bool(
                get_char(ctx, this).map(|c| c.is_ascii()).unwrap_or(false),
            ))
        }

        #[varn_method("isPunctuation")]
        pub fn is_punctuation(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            _args: &[VmValue],
        ) -> NativeFnResult {
            Ok(VmValue::from_bool(
                get_char(ctx, this)
                    .map(|c| c.is_ascii_punctuation())
                    .unwrap_or(false),
            ))
        }

        #[varn_method("toUppercase")]
        pub fn to_uppercase(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            _args: &[VmValue],
        ) -> NativeFnResult {
            if let Some(c) = get_char(ctx, this) {
                let upper: String = c.to_uppercase().collect();
                if let Some(uc) = upper.chars().next() {
                    return Ok(ctx.intern(Value::Char(uc)));
                }
            }
            Ok(this)
        }

        #[varn_method("toLowercase")]
        pub fn to_lowercase(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            _args: &[VmValue],
        ) -> NativeFnResult {
            if let Some(c) = get_char(ctx, this) {
                let lower: String = c.to_lowercase().collect();
                if let Some(lc) = lower.chars().next() {
                    return Ok(ctx.intern(Value::Char(lc)));
                }
            }
            Ok(this)
        }

        #[varn_static("fromCode")]
        pub fn from_code(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
            if let Some(&v) = args.first() {
                if let Value::Int(n) = ctx.extract(v) {
                    if let Some(c) = char::from_u32(n as u32) {
                        return Ok(ctx.intern(Value::Char(c)));
                    }
                }
            }
            Ok(VmValue::null())
        }
    }
}
