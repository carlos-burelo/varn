#[allow(unused_imports)]
use varn_op_macros::{varn_class, varn_constructor, varn_module};
use varn_types::{NativeCtx, VmValue};

fn init_error(ctx: &mut dyn NativeCtx, this: VmValue, args: &[VmValue], class_name: &'static str) {
    let msg = args.first().copied().unwrap_or(VmValue::null());
    ctx.set_field(this, "message", msg);
    let name = ctx.alloc_str(class_name);
    ctx.set_field(this, "name", name);
    let stack = ctx.alloc_str("");
    ctx.set_field(this, "stack", stack);
}

#[varn_module("globals")]
pub(crate) mod dispatch {
    use super::*;

    #[varn_class("Error")]
    pub mod error_class {
        use super::*;

        #[varn_constructor]
        pub fn constructor(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            args: &[VmValue],
        ) -> Result<(), String> {
            init_error(ctx, this, args, "Error");
            Ok(())
        }
    }

    #[varn_class("TypeError", extends = "Error")]
    pub mod type_error_class {
        use super::*;

        #[varn_constructor]
        pub fn constructor(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            args: &[VmValue],
        ) -> Result<(), String> {
            init_error(ctx, this, args, "TypeError");
            Ok(())
        }
    }

    #[varn_class("RangeError", extends = "Error")]
    pub mod range_error_class {
        use super::*;

        #[varn_constructor]
        pub fn constructor(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            args: &[VmValue],
        ) -> Result<(), String> {
            init_error(ctx, this, args, "RangeError");
            Ok(())
        }
    }
}
