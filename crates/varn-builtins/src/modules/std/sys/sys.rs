use varn_op_macros::varn_module;
use varn_types::{NativeCtx, VmValue};

#[varn_module("std:sys")]
pub(crate) mod dispatch {
    use super::*;

    #[varn_fn]
    pub fn platform(ctx: &mut dyn NativeCtx, _args: &[VmValue]) -> Result<VmValue, String> {
        Ok(ctx.alloc_str(std::env::consts::OS))
    }

    #[varn_fn]
    pub fn cwd(ctx: &mut dyn NativeCtx, _args: &[VmValue]) -> Result<VmValue, String> {
        let cwd = std::env::current_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        Ok(ctx.alloc_str_owned(cwd))
    }

    #[varn_fn]
    pub fn exit(_ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let code = match args.first() {
            Some(v) if v.is_int() => v.as_int() as i32,
            _ => 0,
        };
        std::process::exit(code);
    }

    #[varn_fn("env", cap = "sys.env.read")]
    pub fn env_get(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        if let Some(&v) = args.first() {
            if let Some(key) = ctx.str_owned(v) {
                return Ok(std::env::var(key)
                    .map(|val| ctx.alloc_str_owned(val))
                    .unwrap_or(VmValue::null()));
            }
        }
        Ok(VmValue::null())
    }
}
