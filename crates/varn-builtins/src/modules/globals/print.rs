use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use varn_op_macros::varn_module;
use varn_types::{NativeCtx, VmValue};

static SILENT: AtomicBool = AtomicBool::new(false);

pub fn set_print_silent(silent: bool) {
    SILENT.store(silent, Ordering::Relaxed);
}

#[varn_module("print")]
pub(crate) mod dispatch {
    use super::*;

    #[varn_fn]
    pub fn print(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let s = args
            .iter()
            .map(|&v| ctx.str_repr(v))
            .collect::<Vec<_>>()
            .join(" ");
        if !SILENT.load(Ordering::Relaxed) {
            let stdout = std::io::stdout();
            let mut out = stdout.lock();
            let _ = writeln!(out, "{s}");
            let _ = out.flush();
        }
        Ok(VmValue::null())
    }

    #[varn_fn]
    pub fn debug(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let s = args
            .iter()
            .map(|&v| ctx.str_repr(v))
            .collect::<Vec<_>>()
            .join(" ");
        eprintln!("[debug] {s}");
        Ok(VmValue::null())
    }
}

pub use dispatch::{debug, print};
