#[allow(unused_imports)]
use varn_op_macros::{varn_class, varn_constructor, varn_module, varn_static_getter};
use varn_types::value::RuntimeSymbol;
use varn_types::{NativeCtx, Value, VmValue};

#[varn_module("globals")]
pub(crate) mod dispatch {
    use super::*;

    #[varn_class("Symbol")]
    pub mod symbol_class {
        use super::*;

        #[varn_static_getter("iterator")]
        pub fn iterator(ctx: &mut dyn NativeCtx) -> Result<VmValue, String> {
            Ok(ctx.intern(Value::Symbol(RuntimeSymbol::Iterator)))
        }

        #[varn_static_getter("asyncIterator")]
        pub fn async_iterator(ctx: &mut dyn NativeCtx) -> Result<VmValue, String> {
            Ok(ctx.intern(Value::Symbol(RuntimeSymbol::AsyncIterator)))
        }

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
