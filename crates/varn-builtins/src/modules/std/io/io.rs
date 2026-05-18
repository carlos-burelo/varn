use std::io::Write as IoWrite;
use varn_op_macros::varn_module;
use varn_types::{NativeCtx, VmValue};

#[varn_module("std:io")]
pub(crate) mod dispatch {
    use super::*;

    #[varn_fn(cap = "io.write")]
    pub fn write(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        for &v in args {
            print!("{}", ctx.str_repr(v));
        }
        Ok(VmValue::null())
    }

    #[varn_fn(cap = "io.write")]
    pub fn writeln(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        for &v in args {
            print!("{}", ctx.str_repr(v));
        }
        println!();
        Ok(VmValue::null())
    }

    #[varn_fn(cap = "io.write")]
    pub fn flush(_ctx: &mut dyn NativeCtx, _args: &[VmValue]) -> Result<VmValue, String> {
        std::io::stdout()
            .flush()
            .map_err(|e| format!("io.flush: {e}"))?;
        Ok(VmValue::null())
    }

    #[varn_fn("readLine", cap = "io.read")]
    pub fn read_line(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        if let Some(&prompt) = args.first() {
            if !prompt.is_null() {
                print!("{}", ctx.str_repr(prompt));
                let _ = std::io::stdout().flush();
            }
        }
        let mut line = String::new();
        std::io::stdin()
            .read_line(&mut line)
            .map_err(|e| format!("io.read_line: {e}"))?;

        let trimmed = line.trim_end_matches(['\r', '\n']).to_string();
        Ok(ctx.alloc_str_owned(trimmed))
    }
}
