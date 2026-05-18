#[allow(unused_imports)]
use varn_op_macros::{varn_class, varn_constructor, varn_method, varn_module, varn_static};
use varn_types::{NativeCtx, NativeFnResult, VmValue};

#[varn_module("globals")]
pub(crate) mod dispatch {
    use super::*;

    #[varn_class("bool")]
    pub mod bool_class {
        use super::*;

        #[varn_method("toString")]
        pub fn to_str(ctx: &mut dyn NativeCtx, this: VmValue, _args: &[VmValue]) -> NativeFnResult {
            Ok(ctx.alloc_str(if this.as_bool() { "true" } else { "false" }))
        }
    }
}

pub fn boolean_to_string(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
    let v = args.first().copied().unwrap_or(VmValue::null());
    Ok(ctx.alloc_str(if v.as_bool() { "true" } else { "false" }))
}
