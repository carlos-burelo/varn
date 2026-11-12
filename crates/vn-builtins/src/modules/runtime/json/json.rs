use vn_core::StdErrorClass;
use vn_macros::vn_contract;
use vn_types::NativeError;
use vn_types::{NativeCtx, VmValue};

pub struct JsonRuntime;

vn_contract! {
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
