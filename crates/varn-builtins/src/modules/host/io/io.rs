use std::io::Write as IoWrite;
use varn_op_macros::varn_contract;
use varn_types::{NativeCtx, VmValue};

pub struct IoRuntime;

varn_contract! {
    module: "runtime:io",
    contract: "src/modules/host/io/io_runtime.vn",
    impl IoRuntime {
        fn write(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<(), String> {
            if !crate::modules::globals::is_print_silent() {
                for &v in args {
                    print!("{}", ctx.str_repr_borrowed(v));
                }
            }
            Ok(())
        }

        fn writeln(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<(), String> {
            if !crate::modules::globals::is_print_silent() {
                for &v in args {
                    print!("{}", ctx.str_repr_borrowed(v));
                }
                println!();
            }
            Ok(())
        }

        fn flush(_ctx: &mut dyn NativeCtx) -> Result<(), String> {
            if !crate::modules::globals::is_print_silent() {
                std::io::stdout().flush().map_err(|e| format!("io.flush: {e}"))?;
            }
            Ok(())
        }

        fn readLine(_ctx: &mut dyn NativeCtx, prompt: Option<&str>) -> Result<String, String> {
            if let Some(p) = prompt {
                if !crate::modules::globals::is_print_silent() {
                    print!("{p}");
                    let _ = std::io::stdout().flush();
                }
            }
            let mut line = String::new();
            std::io::stdin()
                .read_line(&mut line)
                .map_err(|e| format!("io.read_line: {e}"))?;
            Ok(line.trim_end_matches(['\r', '\n']).to_string())
        }
    }
}

pub fn read_line(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
    if let Some(&p) = args.first() {
        if !p.is_null() && !crate::modules::globals::is_print_silent() {
            print!("{}", ctx.str_repr_borrowed(p));
            let _ = std::io::stdout().flush();
        }
    }
    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .map_err(|e| format!("io.read_line: {e}"))?;
    Ok(ctx.alloc_str_owned(line.trim_end_matches(['\r', '\n']).to_string()))
}
