#[allow(unused_imports)]
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
#[allow(unused_imports)]
use varn_op_macros::{varn_class, varn_constructor, varn_method, varn_module};
use varn_types::{NativeCtx, VmValue};

static SILENT: AtomicBool = AtomicBool::new(false);

pub fn set_print_silent(silent: bool) {
    SILENT.store(silent, Ordering::Relaxed);
}

pub fn is_print_silent() -> bool {
    SILENT.load(Ordering::Relaxed)
}

fn init_error(ctx: &mut dyn NativeCtx, this: VmValue, args: &[VmValue], class_name: &'static str) {
    let msg = args.first().copied().unwrap_or(VmValue::null());
    ctx.set_field(this, "message", msg);
    let name = ctx.alloc_str(class_name);
    ctx.set_field(this, "name", name);
    let stack = ctx.alloc_str("");
    ctx.set_field(this, "stack", stack);
}

#[varn_module("globals")]
pub(crate) mod dispatch {
    use super::*;

    #[varn_fn]
    pub fn print(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        if !SILENT.load(Ordering::Relaxed) {
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
        Ok(VmValue::null())
    }

    #[varn_fn]
    pub fn debug(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let s = args
            .iter()
            .map(|&v| ctx.str_repr_borrowed(v))
            .collect::<Vec<_>>()
            .join(" ");
        varn_utilities::terminal::tagged("debug", s);
        Ok(VmValue::null())
    }

    #[varn_fn]
    pub fn input(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        crate::modules::io::read_line(ctx, args)
    }

    #[varn_fn("assertSummary")]
    pub fn assert_summary(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        crate::modules::testing::dispatch::summary(ctx, args)
    }

    #[varn_fn]
    pub fn assert(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let cond = unsafe { args.get_unchecked(1).is_truthy() };
        if cond {
            crate::modules::testing::inc_passed();
            Ok(VmValue::null())
        } else {
            let label = unsafe { ctx.str_repr(*args.get_unchecked(0)) };
            crate::modules::testing::inc_failed();
            varn_utilities::terminal::error(format!("ASSERT FAIL: {label}"));
            Err(label)
        }
    }

    #[varn_class("Error")]
    pub mod error_class {
        use super::*;

        #[varn_constructor]
        pub fn constructor(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            args: &[VmValue],
        ) -> Result<(), String> {
            init_error(ctx, this, args, "Error");
            Ok(())
        }

        #[varn_method("toString")]
        pub fn to_string(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            _args: &[VmValue],
        ) -> Result<VmValue, String> {
            let name = ctx
                .get_field(this, "name")
                .and_then(|v| ctx.str_owned(v))
                .unwrap_or_else(|| "Error".to_owned());
            let message = ctx
                .get_field(this, "message")
                .and_then(|v| ctx.str_owned(v))
                .unwrap_or_default();
            let rendered = if message.is_empty() {
                name
            } else {
                format!("{name}: {message}")
            };
            Ok(ctx.alloc_str_owned(rendered))
        }
    }

    #[varn_class("TypeError", extends = "Error")]
    pub mod type_error_class {
        use super::*;

        #[varn_constructor]
        pub fn constructor(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            args: &[VmValue],
        ) -> Result<(), String> {
            init_error(ctx, this, args, "TypeError");
            Ok(())
        }
    }

    #[varn_class("RangeError", extends = "Error")]
    pub mod range_error_class {
        use super::*;

        #[varn_constructor]
        pub fn constructor(
            ctx: &mut dyn NativeCtx,
            this: VmValue,
            args: &[VmValue],
        ) -> Result<(), String> {
            init_error(ctx, this, args, "RangeError");
            Ok(())
        }
    }
}
