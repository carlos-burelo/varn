use varn_op_macros::varn_module;
use varn_types::{NativeCtx, VmValue};

#[varn_module("globals")]
pub(crate) mod dispatch {
    use super::*;

    #[varn_fn]
    pub fn print(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        crate::modules::print::print(ctx, args)
    }

    #[varn_fn]
    pub fn debug(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        crate::modules::print::debug(ctx, args)
    }

    #[varn_fn]
    pub fn input(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        crate::modules::io::dispatch::read_line(ctx, args)
    }

    #[varn_fn]
    pub fn range(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        crate::modules::primitives::range::range_op(ctx, args)
    }

    #[varn_fn("assertSummary")]
    pub fn assert_summary(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        crate::modules::testing::dispatch::test_group::summary(ctx, args)
    }

    #[varn_fn]
    pub fn assert(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let label = args
            .get(0)
            .map(|&v| ctx.str_repr(v))
            .unwrap_or_else(|| "assert failed".to_owned());
        let cond = args.get(1).map(|&v| v.is_truthy()).unwrap_or(false);
        if cond {
            crate::modules::testing::inc_passed();
            Ok(VmValue::null())
        } else {
            crate::modules::testing::inc_failed();
            eprintln!("ASSERT FAIL: {label}");
            Err(label)
        }
    }
}
