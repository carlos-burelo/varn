use std::sync::atomic::{AtomicBool, Ordering};
use varn_op_macros::varn_contract;
use varn_types::{NativeCtx, VmValue};

static SILENT: AtomicBool = AtomicBool::new(false);

pub fn set_print_silent(silent: bool) {
    SILENT.store(silent, Ordering::Relaxed);
}

pub fn is_print_silent() -> bool {
    SILENT.load(Ordering::Relaxed)
}

pub struct Globals;

varn_contract! {
    module: "globals",
    contract: "src/modules/core/globals/globals.vn",
    impl Globals {
        fn print(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<(), String> {
            if !SILENT.load(Ordering::Relaxed) {
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

        fn debug(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<(), String> {
            let s = args
                .iter()
                .map(|&v| ctx.str_repr_borrowed(v))
                .collect::<Vec<_>>()
                .join(" ");
            varn_core::term::terminal::tagged("debug", s);
            Ok(())
        }

        fn assert(_ctx: &mut dyn NativeCtx, label: &str, cond: bool) -> Result<(), String> {
            if cond {
                crate::modules::testing::inc_passed();
                Ok(())
            } else {
                crate::modules::testing::inc_failed();
                varn_core::term::terminal::error(format!("ASSERT FAIL: {label}"));
                Err(label.to_string())
            }
        }

        fn assertSummary(ctx: &mut dyn NativeCtx) -> Result<(), String> {
            crate::modules::testing::summary(ctx, &[])?;
            Ok(())
        }

        fn input(_ctx: &mut dyn NativeCtx, prompt: Option<&str>) -> Result<String, String> {
            use std::io::Write;
            if let Some(p) = prompt {
                if !is_print_silent() {
                    print!("{p}");
                    let _ = std::io::stdout().flush();
                }
            }
            let mut line = String::new();
            std::io::stdin()
                .read_line(&mut line)
                .map_err(|e| format!("input: {e}"))?;
            Ok(line.trim_end_matches(['\r', '\n']).to_string())
        }
    }
}
