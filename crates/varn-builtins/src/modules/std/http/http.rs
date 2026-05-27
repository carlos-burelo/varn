use varn_op_macros::varn_module;
use varn_types::{NativeCtx, VmValue};

#[varn_module("runtime:http")]
pub(crate) mod dispatch {
    use super::*;

    #[varn_fn("httpFetch", cap = "network.client")]
    pub fn fetch(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let url = args.first().map(|&v| ctx.str_repr(v)).unwrap_or_default();
        Ok(ctx.alloc_str_owned(format!("fetch result for {}", url)))
    }

    #[varn_fn("httpCreateServer", cap = "network.server")]
    pub fn create_server(_ctx: &mut dyn NativeCtx, _args: &[VmValue]) -> Result<VmValue, String> {
        Ok(VmValue::from_int(0))
    }

    #[varn_fn("httpAddRoute", cap = "network.server")]
    pub fn add_route(_ctx: &mut dyn NativeCtx, _args: &[VmValue]) -> Result<VmValue, String> {
        Ok(VmValue::null())
    }

    #[varn_fn("httpListen", cap = "network.server")]
    pub fn listen(_ctx: &mut dyn NativeCtx, _args: &[VmValue]) -> Result<VmValue, String> {
        Ok(VmValue::null())
    }

    #[varn_fn("httpSendResponse", cap = "network.server")]
    pub fn send_response(_ctx: &mut dyn NativeCtx, _args: &[VmValue]) -> Result<VmValue, String> {
        Ok(VmValue::null())
    }
}
