use url::Url;
use varn_op_macros::varn_module;
use varn_types::{NativeCtx, VmValue};

#[varn_module("std:net", cap = "net.client")]
pub(crate) mod dispatch {
    use super::*;

    #[varn_fn("isIP")]
    pub fn net_is_ip(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let s = args.first().map(|&v| ctx.str_repr(v)).unwrap_or_default();
        Ok(VmValue::from_bool(s.parse::<std::net::IpAddr>().is_ok()))
    }

    #[varn_fn("parseURL")]
    pub fn parse_url(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let s = args.first().map(|&v| ctx.str_repr(v)).unwrap_or_default();
        let url = Url::parse(&s).map_err(|e| format!("Net.parseURL: {e}"))?;

        let obj = ctx.alloc_object();
        let proto = ctx.alloc_str_owned(format!("{}:", url.scheme()));
        ctx.set_field(obj, "protocol", proto);
        let host = ctx.alloc_str(url.host_str().unwrap_or(""));
        ctx.set_field(obj, "host", host);
        let path = ctx.alloc_str(url.path());
        ctx.set_field(obj, "path", path);

        Ok(obj)
    }
}
