use url::Url;
use urlencoding::{decode, encode};
use varn_op_macros::varn_module;
use varn_types::{NativeCtx, VmValue};

#[varn_module("runtime:net")]
pub(crate) mod dispatch {
    use super::*;

    #[varn_fn("netIsIP")]
    pub fn is_ip(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let s = args.first().map(|&v| ctx.str_repr(v)).unwrap_or_default();
        Ok(VmValue::from_bool(s.parse::<std::net::IpAddr>().is_ok()))
    }

    #[varn_fn("netIsIPv4")]
    pub fn is_ipv4(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let s = args.first().map(|&v| ctx.str_repr(v)).unwrap_or_default();
        Ok(VmValue::from_bool(s.parse::<std::net::Ipv4Addr>().is_ok()))
    }

    #[varn_fn("netIsIPv6")]
    pub fn is_ipv6(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let s = args.first().map(|&v| ctx.str_repr(v)).unwrap_or_default();
        Ok(VmValue::from_bool(s.parse::<std::net::Ipv6Addr>().is_ok()))
    }

    #[varn_fn("netParseUrl")]
    pub fn parse_url(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let s = args.first().map(|&v| ctx.str_repr(v)).unwrap_or_default();
        let url = Url::parse(&s).map_err(|e| format!("Net.parseURL: {e}"))?;

        let obj = ctx.alloc_object();
        let proto = ctx.alloc_str(url.scheme());
        ctx.set_field(obj, "protocol", proto);
        let host = ctx.alloc_str(url.host_str().unwrap_or(""));
        ctx.set_field(obj, "host", host);
        let port = url
            .port()
            .map(|p| VmValue::from_int(p as i64))
            .unwrap_or(VmValue::null());
        ctx.set_field(obj, "port", port);
        let path = ctx.alloc_str(url.path());
        ctx.set_field(obj, "path", path);
        let query = ctx.alloc_str(url.query().unwrap_or(""));
        ctx.set_field(obj, "query", query);
        let fragment = ctx.alloc_str(url.fragment().unwrap_or(""));
        ctx.set_field(obj, "fragment", fragment);

        Ok(obj)
    }

    #[varn_fn("netEncodeURIComponent")]
    pub fn encode_uri_component(
        ctx: &mut dyn NativeCtx,
        args: &[VmValue],
    ) -> Result<VmValue, String> {
        let s = args.first().map(|&v| ctx.str_repr(v)).unwrap_or_default();
        Ok(ctx.alloc_str_owned(encode(&s).into_owned()))
    }

    #[varn_fn("netDecodeURIComponent")]
    pub fn decode_uri_component(
        ctx: &mut dyn NativeCtx,
        args: &[VmValue],
    ) -> Result<VmValue, String> {
        let s = args.first().map(|&v| ctx.str_repr(v)).unwrap_or_default();
        let decoded = decode(&s).map_err(|e| format!("Net.decodeURIComponent: {e}"))?;
        Ok(ctx.alloc_str_owned(decoded.into_owned()))
    }
}
