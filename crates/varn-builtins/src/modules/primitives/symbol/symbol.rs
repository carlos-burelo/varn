use varn_op_macros::varn_contract;
use varn_types::value::RuntimeSymbol;
use varn_types::{NativeCtx, Value, VmValue};

pub struct Symbol;

varn_contract! {
    module: "globals",
    class: "Symbol",
    contract: "src/modules/primitives/symbol/symbol.vn",
    impl Symbol {
        fn iterator(ctx: &mut dyn NativeCtx) -> VmValue {
            ctx.intern(Value::Symbol(RuntimeSymbol::Iterator))
        }
        fn asyncIterator(ctx: &mut dyn NativeCtx) -> VmValue {
            ctx.intern(Value::Symbol(RuntimeSymbol::AsyncIterator))
        }

        fn description(ctx: &mut dyn NativeCtx, this: VmValue) -> Option<String> {
            ctx.get_field(this, "description")
                .filter(|v| !v.is_null())
                .and_then(|v| ctx.str_owned(v))
        }

        fn constructor(ctx: &mut dyn NativeCtx, this: VmValue, description: Option<&str>) -> VmValue {
            let desc = match description {
                Some(s) => ctx.alloc_str(s),
                None => VmValue::null(),
            };
            ctx.set_field(this, "description", desc);
            this
        }
    }
}
