use varn_op_macros::varn_contract;
use varn_types::{NativeCtx, VmValue};

pub struct JsonRuntime;

varn_contract! {
    module: "runtime:json",
    contract: "src/modules/host/json/json_runtime.vn",
    impl JsonRuntime {
        fn parse(ctx: &mut dyn NativeCtx, text: &str) -> Result<VmValue, String> {
            ctx.parse_json(text)
        }

        fn stringify(ctx: &mut dyn NativeCtx, value: VmValue) -> Result<String, String> {
            ctx.stringify_json(value)
        }
    }
}
