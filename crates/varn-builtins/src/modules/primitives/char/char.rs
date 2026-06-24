use varn_op_macros::varn_contract;
use varn_types::{NativeCtx, Value, VmValue};

pub struct Char;

varn_contract! {
    module: "globals",
    class: "char",
    contract: "src/modules/primitives/char/char.vn",
    impl Char {
        fn fromCode(_ctx: &mut dyn NativeCtx, code: i64) -> char {
            char::from_u32(code as u32).unwrap_or(char::REPLACEMENT_CHARACTER)
        }

        fn toString(_ctx: &mut dyn NativeCtx, this: char) -> String { this.to_string() }
        fn toStr(_ctx: &mut dyn NativeCtx, this: char) -> String { this.to_string() }
        fn charCodeAt(_ctx: &mut dyn NativeCtx, this: char) -> i64 { this as i64 }

        fn isAlphabetic(_ctx: &mut dyn NativeCtx, this: char) -> bool { this.is_alphabetic() }
        fn isAlphanumeric(_ctx: &mut dyn NativeCtx, this: char) -> bool { this.is_alphanumeric() }
        fn isDigit(_ctx: &mut dyn NativeCtx, this: char) -> bool { this.is_ascii_digit() }
        fn isWhitespace(_ctx: &mut dyn NativeCtx, this: char) -> bool { this.is_whitespace() }
        fn isUppercase(_ctx: &mut dyn NativeCtx, this: char) -> bool { this.is_uppercase() }
        fn isLowercase(_ctx: &mut dyn NativeCtx, this: char) -> bool { this.is_lowercase() }
        fn isAscii(_ctx: &mut dyn NativeCtx, this: char) -> bool { this.is_ascii() }
        fn isPunctuation(_ctx: &mut dyn NativeCtx, this: char) -> bool { this.is_ascii_punctuation() }

        fn toUppercase(ctx: &mut dyn NativeCtx, this: char) -> VmValue {
            let u = this.to_uppercase().next().unwrap_or(this);
            ctx.intern(Value::Char(u))
        }
        fn toLowercase(ctx: &mut dyn NativeCtx, this: char) -> VmValue {
            let l = this.to_lowercase().next().unwrap_or(this);
            ctx.intern(Value::Char(l))
        }
    }
}
