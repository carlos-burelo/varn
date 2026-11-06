use varn_core::RuntimeErrorKind;
use varn_macros::varn_contract;
use varn_types::{NativeCtx, VmValue};

fn init_error(ctx: &mut dyn NativeCtx, this: VmValue, message: Option<&str>, class_name: &str) {
    let msg = match message {
        Some(m) => ctx.alloc_str(m),
        None => VmValue::null(),
    };
    ctx.set_field(this, varn_core::MemberKey::Message.as_str(), msg);
    let name = ctx.alloc_str(class_name);
    ctx.set_field(this, varn_core::MemberKey::Name.as_str(), name);
    let stack = ctx.alloc_str("");
    ctx.set_field(this, varn_core::MemberKey::Stack.as_str(), stack);
}

pub struct ErrorClass;

pub struct TypeErrorClass;

pub struct RangeErrorClass;

pub struct IntegerOverflowClass;

pub struct DivisionByZeroClass;

pub struct MatchErrorClass;

varn_contract! {
    module: "globals",
    contract: "src/modules/core/errors/errors.vn",
    class: "Error",
    impl ErrorClass {
        fn constructor(ctx: &mut dyn NativeCtx, this: VmValue, message: Option<&str>) -> VmValue {
            init_error(ctx, this, message, RuntimeErrorKind::Error.class_name());
            this
        }

        fn toString(ctx: &mut dyn NativeCtx, this: VmValue) -> String {
            let name = ctx
                .get_field(this, varn_core::MemberKey::Name.as_str())
                .and_then(|v| ctx.str_owned(v))
                .unwrap_or_else(|| RuntimeErrorKind::Error.class_name().to_owned());
            let message = ctx
                .get_field(this, varn_core::MemberKey::Message.as_str())
                .and_then(|v| ctx.str_owned(v))
                .unwrap_or_default();
            if message.is_empty() {
                name
            } else {
                format!("{name}: {message}")
            }
        }
    }

    class: "TypeError",
    impl TypeErrorClass {
        fn constructor(ctx: &mut dyn NativeCtx, this: VmValue, message: Option<&str>) -> VmValue {
            init_error(ctx, this, message, RuntimeErrorKind::TypeError.class_name());
            this
        }
    }

    class: "RangeError",
    impl RangeErrorClass {
        fn constructor(ctx: &mut dyn NativeCtx, this: VmValue, message: Option<&str>) -> VmValue {
            init_error(ctx, this, message, RuntimeErrorKind::RangeError.class_name());
            this
        }
    }

    class: "IntegerOverflow",
    impl IntegerOverflowClass {
        fn constructor(ctx: &mut dyn NativeCtx, this: VmValue, message: Option<&str>) -> VmValue {
            init_error(ctx, this, message, RuntimeErrorKind::IntegerOverflow.class_name());
            this
        }
    }

    class: "DivisionByZero",
    impl DivisionByZeroClass {
        fn constructor(ctx: &mut dyn NativeCtx, this: VmValue, message: Option<&str>) -> VmValue {
            init_error(ctx, this, message, RuntimeErrorKind::DivisionByZero.class_name());
            this
        }
    }

    class: "MatchError",
    impl MatchErrorClass {
        fn constructor(ctx: &mut dyn NativeCtx, this: VmValue, message: Option<&str>) -> VmValue {
            init_error(ctx, this, message, RuntimeErrorKind::MatchError.class_name());
            this
        }
    }
}
