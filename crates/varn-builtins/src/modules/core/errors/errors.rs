use varn_op_macros::varn_contract;
use varn_types::{NativeCtx, VmValue};

fn init_error(ctx: &mut dyn NativeCtx, this: VmValue, message: Option<&str>, class_name: &str) {
    let msg = match message {
        Some(m) => ctx.alloc_str(m),
        None => VmValue::null(),
    };
    ctx.set_field(this, "message", msg);
    let name = ctx.alloc_str(class_name);
    ctx.set_field(this, "name", name);
    let stack = ctx.alloc_str("");
    ctx.set_field(this, "stack", stack);
}

pub struct ErrorClass;

varn_contract! {
    module: "globals",
    class: "Error",
    contract: "src/modules/core/errors/errors.vn",
    impl ErrorClass {
        fn constructor(ctx: &mut dyn NativeCtx, this: VmValue, message: Option<&str>) -> VmValue {
            init_error(ctx, this, message, "Error");
            this
        }

        fn toString(ctx: &mut dyn NativeCtx, this: VmValue) -> String {
            let name = ctx
                .get_field(this, "name")
                .and_then(|v| ctx.str_owned(v))
                .unwrap_or_else(|| "Error".to_owned());
            let message = ctx
                .get_field(this, "message")
                .and_then(|v| ctx.str_owned(v))
                .unwrap_or_default();
            if message.is_empty() {
                name
            } else {
                format!("{name}: {message}")
            }
        }
    }
}

pub struct TypeErrorClass;

varn_contract! {
    module: "globals",
    class: "TypeError",
    extends: "Error",
    contract: "src/modules/core/errors/errors.vn",
    impl TypeErrorClass {
        fn constructor(ctx: &mut dyn NativeCtx, this: VmValue, message: Option<&str>) -> VmValue {
            init_error(ctx, this, message, "TypeError");
            this
        }
    }
}

pub struct RangeErrorClass;

varn_contract! {
    module: "globals",
    class: "RangeError",
    extends: "Error",
    contract: "src/modules/core/errors/errors.vn",
    impl RangeErrorClass {
        fn constructor(ctx: &mut dyn NativeCtx, this: VmValue, message: Option<&str>) -> VmValue {
            init_error(ctx, this, message, "RangeError");
            this
        }
    }
}

pub struct IntegerOverflowClass;

varn_contract! {
    module: "globals",
    class: "IntegerOverflow",
    extends: "Error",
    contract: "src/modules/core/errors/errors.vn",
    impl IntegerOverflowClass {
        fn constructor(ctx: &mut dyn NativeCtx, this: VmValue, message: Option<&str>) -> VmValue {
            init_error(ctx, this, message, "IntegerOverflow");
            this
        }
    }
}

pub struct DivisionByZeroClass;

varn_contract! {
    module: "globals",
    class: "DivisionByZero",
    extends: "Error",
    contract: "src/modules/core/errors/errors.vn",
    impl DivisionByZeroClass {
        fn constructor(ctx: &mut dyn NativeCtx, this: VmValue, message: Option<&str>) -> VmValue {
            init_error(ctx, this, message, "DivisionByZero");
            this
        }
    }
}

pub struct MatchErrorClass;

varn_contract! {
    module: "globals",
    class: "MatchError",
    extends: "Error",
    contract: "src/modules/core/errors/errors.vn",
    impl MatchErrorClass {
        fn constructor(ctx: &mut dyn NativeCtx, this: VmValue, message: Option<&str>) -> VmValue {
            init_error(ctx, this, message, "MatchError");
            this
        }
    }
}
