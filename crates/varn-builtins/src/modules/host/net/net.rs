use url::Url;
use urlencoding::{decode, encode};
use varn_op_macros::varn_contract;
use varn_types::{NativeCtx, VmValue};

pub struct NetRuntime;

varn_contract! {
    module: "runtime:net",
    contract: "src/modules/host/net/net_runtime.vn",
    impl NetRuntime {
        fn isIP(_ctx: &mut dyn NativeCtx, s: &str) -> Result<bool, String> {
            Ok(s.parse::<std::net::IpAddr>().is_ok())
        }
        fn isIPv4(_ctx: &mut dyn NativeCtx, s: &str) -> Result<bool, String> {
            Ok(s.parse::<std::net::Ipv4Addr>().is_ok())
        }
        fn isIPv6(_ctx: &mut dyn NativeCtx, s: &str) -> Result<bool, String> {
            Ok(s.parse::<std::net::Ipv6Addr>().is_ok())
        }
        fn parseUrl(ctx: &mut dyn NativeCtx, url: &str) -> Result<VmValue, String> {
            let url = Url::parse(url).map_err(|e| format!("Net.parseURL: {e}"))?;
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
        fn encodeURIComponent(_ctx: &mut dyn NativeCtx, value: &str) -> Result<String, String> {
            Ok(encode(value).into_owned())
        }
        fn decodeURIComponent(_ctx: &mut dyn NativeCtx, value: &str) -> Result<String, String> {
            decode(value)
                .map(|d| d.into_owned())
                .map_err(|e| format!("Net.decodeURIComponent: {e}"))
        }
    }
}
