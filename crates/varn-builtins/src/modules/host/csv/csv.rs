use varn_op_macros::varn_contract;
use varn_types::{NativeCtx, VmValue};

pub struct CsvRuntime;

varn_contract! {
    module: "runtime:csv",
    contract: "src/modules/host/csv/csv_runtime.vn",
    impl CsvRuntime {
        fn parse(ctx: &mut dyn NativeCtx, text: &str, delimiter: &str, has_header: bool, trim: bool) -> Result<VmValue, String> {
            let delim_byte = delimiter.as_bytes().first().copied().unwrap_or(b',');
            ctx.parse_csv(text, delim_byte, has_header, trim)
        }

        fn stringify(ctx: &mut dyn NativeCtx, data: VmValue, delimiter: &str) -> Result<String, String> {
            let delim_byte = delimiter.as_bytes().first().copied().unwrap_or(b',');
            ctx.stringify_csv(data, delim_byte)
        }
    }
}
