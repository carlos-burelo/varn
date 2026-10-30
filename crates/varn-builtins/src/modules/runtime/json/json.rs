use varn_core::StdErrorClass;
use varn_macros::varn_contract;
use varn_types::NativeError;
use varn_types::{NativeCtx, VmValue};

pub struct JsonRuntime;

varn_contract! {
    module: "runtime:json",
    contract: "src/modules/runtime/json/json_runtime.vn",
    impl JsonRuntime {
        fn parse(ctx: &mut dyn NativeCtx, text: &str) -> Result<VmValue, NativeError> {
            (ctx.parse_json(text)).map_err(|e| NativeError::std(StdErrorClass::Parse, e))
        }

        fn stringify(ctx: &mut dyn NativeCtx, value: VmValue) -> Result<String, NativeError> {
            (ctx.stringify_json(value)).map_err(NativeError::range_error)
        }
    }
}
