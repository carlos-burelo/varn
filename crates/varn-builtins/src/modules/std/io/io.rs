use std::io::Write as IoWrite;
use varn_op_macros::varn_contract;
use varn_types::{NativeCtx, VmValue};

/// Native implementation backing the `runtime:io` contract
/// (`src/modules/std/io/runtime/io_runtime.vn`).
pub struct IoRuntime;

varn_contract! {
    module: "runtime:io",
    contract: "src/modules/std/io/runtime/io_runtime.vn",
    impl IoRuntime {
        fn ioWrite(ctx: &mut dyn NativeCtx, args: &[VmValue]) {
            if !crate::modules::globals::is_print_silent() {
                for &v in args {
                    print!("{}", ctx.str_repr_borrowed(v));
                }
            }
        }

        fn ioWriteln(ctx: &mut dyn NativeCtx, args: &[VmValue]) {
            if !crate::modules::globals::is_print_silent() {
                for &v in args {
                    print!("{}", ctx.str_repr_borrowed(v));
                }
                println!();
            }
        }

        fn ioFlush(_ctx: &mut dyn NativeCtx) {
            if !crate::modules::globals::is_print_silent() {
                let _ = std::io::stdout().flush();
            }
        }

        fn ioReadLine(_ctx: &mut dyn NativeCtx, prompt: Option<&str>) -> String {
            if let Some(p) = prompt {
                if !crate::modules::globals::is_print_silent() {
                    print!("{p}");
                    let _ = std::io::stdout().flush();
                }
            }
            let mut line = String::new();
            let _ = std::io::stdin().read_line(&mut line);
            line.trim_end_matches(['\r', '\n']).to_string()
        }
    }
}

/// Stable entrypoint for the global `input()` builtin (raw `&[VmValue]` form).
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
