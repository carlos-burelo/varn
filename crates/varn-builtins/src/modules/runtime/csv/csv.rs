use varn_core::StdErrorClass;
use varn_macros::varn_contract;
use varn_types::NativeError;
use varn_types::{NativeCtx, VmValue};

pub struct CsvRuntime;

varn_contract! {
    module: "runtime:csv",
    contract: "src/modules/runtime/csv/csv_runtime.vn",
    impl CsvRuntime {
        fn parse(ctx: &mut dyn NativeCtx, text: &str, delimiter: &str, has_header: bool, trim: bool) -> Result<VmValue, NativeError> {
            let delim_byte = delimiter.as_bytes().first().copied().unwrap_or(b',');
            (ctx.parse_csv(text, delim_byte, has_header, trim)).map_err(|e| NativeError::std(StdErrorClass::Parse, e))
        }

        fn stringify(ctx: &mut dyn NativeCtx, data: VmValue, delimiter: &str) -> Result<String, NativeError> {
            let delim_byte = delimiter.as_bytes().first().copied().unwrap_or(b',');
            (ctx.stringify_csv(data, delim_byte)).map_err(NativeError::range_error)
        }
    }
}
