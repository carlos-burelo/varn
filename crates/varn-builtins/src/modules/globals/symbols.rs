#[allow(unused_imports)]
use varn_op_macros::{varn_class, varn_constructor, varn_module};
use varn_types::{NativeCtx, VmValue};

pub fn symbol_iterator(_ctx: &mut dyn NativeCtx, _args: &[VmValue]) -> Result<VmValue, String> {
    Ok(VmValue::null())
}

pub fn symbol_async_iterator(
    _ctx: &mut dyn NativeCtx,
    _args: &[VmValue],
) -> Result<VmValue, String> {
    Ok(VmValue::null())
}

#[varn_module("globals")]
mod globals_export {
    use super::*;

    #[varn_class("Symbol")]
    pub mod symbol_class {
        use super::*;

        #[varn_constructor]
        pub fn constructor(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            args: &[VmValue],
        ) -> Result<(), String> {
            let desc = args.first().copied().unwrap_or(VmValue::null());
            ctx.set_field(this, "description", desc);
            Ok(())
        }
    }
}
