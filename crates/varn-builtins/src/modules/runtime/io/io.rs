use varn_core::StdErrorClass;
use std::io::Write as IoWrite;
use varn_macros::varn_contract;
use varn_types::NativeError;
use varn_types::{NativeCtx, VmValue};

pub struct IoRuntime;

varn_contract! {
    module: "runtime:io",
    contract: "src/modules/runtime/io/io_runtime.vn",
    impl IoRuntime {
        fn write(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<(), NativeError> {
            if !ctx.print_silent() {
                for &v in args {
                    print!("{}", ctx.str_repr_borrowed(v));
                }
            }
            Ok(())
        }

        fn writeln(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<(), NativeError> {
            if !ctx.print_silent() {
                for &v in args {
                    print!("{}", ctx.str_repr_borrowed(v));
                }
                println!();
            }
            Ok(())
        }

        fn flush(ctx: &mut dyn NativeCtx) -> Result<(), NativeError> {
            if !ctx.print_silent() {
                std::io::stdout().flush().map_err(|e| format!("E_IO_FLUSH|io.flush: {e}")).map_err(|e| NativeError::std(StdErrorClass::Io, e))?;
            }
            Ok(())
        }

        fn readLine(ctx: &mut dyn NativeCtx, prompt: Option<&str>) -> Result<String, NativeError> {
            if let Some(p) = prompt
                && !ctx.print_silent() {
                    print!("{p}");
                    let _ = std::io::stdout().flush();
                }
            let mut line = String::new();
            std::io::stdin()
                .read_line(&mut line)
                .map_err(|e| format!("E_IO_READ|io.read_line: {e}")).map_err(|e| NativeError::std(StdErrorClass::Io, e))?;
            Ok(line.trim_end_matches(['\r', '\n']).to_string())
        }
    }
}

pub fn read_line(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
    if let Some(&p) = args.first()
        && !p.is_null()
        && !ctx.print_silent()
    {
        print!("{}", ctx.str_repr_borrowed(p));
        let _ = std::io::stdout().flush();
    }
    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .map_err(|e| format!("E_IO_READ|io.read_line: {e}"))?;
    Ok(ctx.alloc_str_owned(line.trim_end_matches(['\r', '\n']).to_string()))
}
