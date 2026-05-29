use varn_op_macros::varn_module;
use varn_types::{NativeCtx, VmValue};

#[varn_module("runtime:task")]
pub(crate) mod dispatch {
    use super::*;

    #[varn_fn("taskSpawn", cap = "async")]
    pub fn spawn(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let first = args
            .first()
            .copied()
            .ok_or("task.spawn: missing function")?;
        ctx.spawn_vm(first, if args.len() > 1 { &args[1..] } else { &[] })
    }

    #[varn_fn("taskSleep", cap = "async")]
    pub fn sleep(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let ms = match args.first() {
            Some(v) if v.is_int() => v.as_int() as u64,
            _ => 0,
        };
        Ok(ctx.suspend_timer(ms))
    }

    #[varn_fn("taskParallel", cap = "async")]
    pub fn parallel(_ctx: &mut dyn NativeCtx, _args: &[VmValue]) -> Result<VmValue, String> {
        Ok(VmValue::null())
    }
}
