use crate::{cli::DisasmArgs, error::CliError, pipeline};

pub fn execute(args: DisasmArgs) -> Result<(), CliError> {
    let debug = crate::opts::parse_debug_opt(args.debug.as_deref())?;
    pipeline::compile_file(&args.file, false, &debug).map(|proto| {
        if !debug.bytecode {
            crate::disasm_impl::print(&proto);
        }
    })
}
