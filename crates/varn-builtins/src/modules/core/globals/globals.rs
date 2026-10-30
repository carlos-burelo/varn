use varn_macros::varn_contract;
use varn_types::NativeError;
use varn_types::{NativeCtx, VmValue};

pub struct Globals;

varn_contract! {
    module: "globals",
    contract: "src/modules/core/globals/globals.vn",
    impl Globals {
        fn sameValue(ctx: &mut dyn NativeCtx, left: VmValue, right: VmValue) -> Result<bool, NativeError> {
            Ok(ctx.values_equal(left, right))
        }

        fn print(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<(), NativeError> {
            if !ctx.print_silent() {
                use std::io::Write;
                let stdout = std::io::stdout();
                let mut out = stdout.lock();
                for (i, &v) in args.iter().enumerate() {
                    if i > 0 {
                        let _ = write!(out, " ");
                    }
                    let _ = write!(out, "{}", ctx.str_repr_borrowed(v));
                }
                let _ = writeln!(out);
                let _ = out.flush();
            }
            Ok(())
        }

        fn debug(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<(), NativeError> {
            let s = args
                .iter()
                .map(|&v| ctx.str_repr_borrowed(v))
                .collect::<Vec<_>>()
                .join(" ");
            varn_core::term::terminal::tagged("debug", s);
            Ok(())
        }

        fn assertSummary(ctx: &mut dyn NativeCtx) -> Result<(), NativeError> {
            crate::modules::testing::summary(ctx, &[]).map_err(NativeError::failure)?;
            Ok(())
        }

        fn input(ctx: &mut dyn NativeCtx, prompt: Option<&str>) -> Result<String, NativeError> {
            use std::io::Write;
            if let Some(p) = prompt
                && !ctx.print_silent() {
                    print!("{p}");
                    let _ = std::io::stdout().flush();
                }
            let mut line = String::new();
            std::io::stdin()
                .read_line(&mut line)
                .map_err(|e| format!("input: {e}")).map_err(NativeError::failure)?;
            Ok(line.trim_end_matches(['\r', '\n']).to_string())
        }
    }
}
