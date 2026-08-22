pub mod driver;
pub mod http_parser;

use driver::driver;
use urlencoding::{decode, encode};
use varn_op_macros::varn_contract;
use varn_types::{NativeCtx, Value, VmValue};

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
        fn encodeURIComponent(_ctx: &mut dyn NativeCtx, value: &str) -> Result<String, String> {
            Ok(encode(value).into_owned())
        }
        fn decodeURIComponent(_ctx: &mut dyn NativeCtx, value: &str) -> Result<String, String> {
            decode(value)
                .map(|d| d.into_owned())
                .map_err(|e| format!("Net.decodeURIComponent: {e}"))
        }

        fn tcpListen(ctx: &mut dyn NativeCtx, port: i64) -> Result<i64, String> {
            if !ctx.check_net_listen(port) {
                return Err(format!("SecurityError: Permission denied (net.server) on port {port}"));
            }
            match driver().listen(port) {
                Ok(id) => Ok(id),
                Err(_) => Ok(-1),
            }
        }

        fn tcpAccept(ctx: &mut dyn NativeCtx, listener_id: i64) -> Result<VmValue, String> {
            let task = driver().accept(listener_id);
            Ok(ctx.intern(Value::TaskHandle(task)))
        }

        fn tcpConnect(ctx: &mut dyn NativeCtx, host: &str, port: i64) -> Result<VmValue, String> {
            if !ctx.check_net_connect(host) {
                return Err(format!("SecurityError: Permission denied (net.client) to host '{host}'"));
            }
            let task = driver().connect(host, port);
            Ok(ctx.intern(Value::TaskHandle(task)))
        }

        fn tcpRead(ctx: &mut dyn NativeCtx, conn_id: i64, len: i64) -> Result<VmValue, String> {
            let task = driver().read(conn_id, len.max(0) as usize);
            Ok(ctx.intern(Value::TaskHandle(task)))
        }

        fn tcpWrite(ctx: &mut dyn NativeCtx, conn_id: i64, data: &str) -> Result<VmValue, String> {
            let task = driver().write(conn_id, data.as_bytes().to_vec());
            Ok(ctx.intern(Value::TaskHandle(task)))
        }

        fn tcpClose(_ctx: &mut dyn NativeCtx, conn_id: i64) -> Result<(), String> {
            driver().close(conn_id);
            Ok(())
        }

        fn tcpCloseListener(_ctx: &mut dyn NativeCtx, listener_id: i64) -> Result<(), String> {
            driver().close_listener(listener_id);
            Ok(())
        }

        fn parseHttpRequest(ctx: &mut dyn NativeCtx, raw: &str) -> Result<VmValue, String> {
            http_parser::parse_http_request(ctx, raw)
        }

        fn sendHttpResponse(
            ctx: &mut dyn NativeCtx,
            conn_id: i64,
            status: i64,
            status_text: &str,
            headers: VmValue,
            body: &str,
        ) -> Result<VmValue, String> {
            use std::io::Write;
            let mut out = Vec::with_capacity(128 + body.len());
            let _ = write!(&mut out, "HTTP/1.1 {status} {status_text}\r\n");

            let mut has_content_length = false;
            let mut has_content_type = false;

            if !headers.is_null() {
                ctx.object_for_each(headers, &mut |key, val| {
                    let lower = key.to_lowercase();
                    if lower == "content-length" {
                        has_content_length = true;
                    } else if lower == "content-type" {
                        has_content_type = true;
                    }
                    if let Some(val_str) = ctx.str_owned(val) {
                        let _ = write!(&mut out, "{key}: {val_str}\r\n");
                    }
                });
            }

            if !has_content_type {
                let _ = write!(&mut out, "Content-Type: text/plain\r\n");
            }

            if !has_content_length {
                let _ = write!(&mut out, "Content-Length: {}\r\n", body.len());
            }

            let _ = write!(&mut out, "\r\n");
            out.extend_from_slice(body.as_bytes());

            let task = driver().write(conn_id, out);
            Ok(ctx.intern(Value::TaskHandle(task)))
        }
    }
}
