pub mod driver;
mod http_head;
pub mod http_parser;

use driver::driver;
use urlencoding::{decode, encode};
use vn_core::{ErrorField, StdErrorClass};
use vn_macros::varn_contract;
use vn_types::NativeError;
use vn_types::{HostOpen, NativeCtx, VmValue};

pub struct NetRuntime;

fn payload(ctx: &mut dyn NativeCtx, data: VmValue) -> Result<Vec<u8>, String> {
    if ctx.is_buffer(data) {
        ctx.buffer_to_bytes(data)
            .ok_or_else(|| "net: could not read the Bytes argument".to_string())
    } else if let Some(text) = ctx.str_owned(data) {
        Ok(text.into_bytes())
    } else {
        Err("net: expected Bytes or str".to_string())
    }
}

varn_contract! {
    module: "runtime:net",
    contract: "src/modules/runtime/net/net_runtime.vn",
    impl NetRuntime {
        fn isIP(_ctx: &mut dyn NativeCtx, s: &str) -> Result<bool, NativeError> {
            Ok(s.parse::<std::net::IpAddr>().is_ok())
        }
        fn isIPv4(_ctx: &mut dyn NativeCtx, s: &str) -> Result<bool, NativeError> {
            Ok(s.parse::<std::net::Ipv4Addr>().is_ok())
        }
        fn isIPv6(_ctx: &mut dyn NativeCtx, s: &str) -> Result<bool, NativeError> {
            Ok(s.parse::<std::net::Ipv6Addr>().is_ok())
        }
        fn encodeURIComponent(_ctx: &mut dyn NativeCtx, value: &str) -> Result<String, NativeError> {
            Ok(encode(value).into_owned())
        }
        fn decodeURIComponent(_ctx: &mut dyn NativeCtx, value: &str) -> Result<String, NativeError> {
            (decode(value)
                .map(|d| d.into_owned())
                .map_err(|e| format!("Net.decodeURIComponent: {e}"))).map_err(|e| NativeError::std(StdErrorClass::Parse, e))
        }

        fn tcpListen(ctx: &mut dyn NativeCtx, port: i64, host: &str) -> Result<i64, NativeError> {
            if !ctx.check_net_listen(port) {
                return Err(NativeError::capability("net.server", port.to_string()));
            }
            driver()
                .listen(port, host)
                .map_err(|e| NativeError::std(StdErrorClass::Net, e.to_string()).with_str(ErrorField::Host, host).with_int(ErrorField::Port, port))
        }

        fn tcpAccept(ctx: &mut dyn NativeCtx, listener_id: i64) -> Result<VmValue, NativeError> {
            let task = driver().accept(listener_id);
            Ok(ctx.task_from_host(task, HostOpen::Plain))
        }

        fn tcpConnect(ctx: &mut dyn NativeCtx, host: &str, port: i64) -> Result<VmValue, NativeError> {
            if !ctx.check_net_connect(host) {
                return Err(NativeError::capability("net.client", host));
            }
            let task = driver().connect(host, port);
            Ok(ctx.task_from_host(task, HostOpen::Plain))
        }

        fn tcpConnectTls(ctx: &mut dyn NativeCtx, host: &str, port: i64, ca: Option<&str>, identity: Option<&str>) -> Result<VmValue, NativeError> {
            if !ctx.check_net_connect(host) {
                return Err(NativeError::capability("net.client", host));
            }
            let task = driver().connect_tls(host, port, ca, identity);
            Ok(ctx.task_from_host(task, HostOpen::Plain))
        }

        fn tcpListenTls(ctx: &mut dyn NativeCtx, port: i64, host: &str, cert: &str, key: &str, client_ca: Option<&str>) -> Result<i64, NativeError> {
            if !ctx.check_net_listen(port) {
                return Err(NativeError::capability("net.server", port.to_string()));
            }
            driver()
                .listen_tls(port, host, cert, key, client_ca)
                .map_err(|e| NativeError::std(StdErrorClass::Net, e).with_str(ErrorField::Host, host).with_int(ErrorField::Port, port))
        }

        fn tcpRead(ctx: &mut dyn NativeCtx, conn_id: i64, len: i64) -> Result<VmValue, NativeError> {
            let task = driver().read(conn_id, len.max(0) as usize);
            Ok(ctx.task_from_host(task, HostOpen::Plain))
        }

        fn tcpWrite(ctx: &mut dyn NativeCtx, conn_id: i64, data: VmValue) -> Result<VmValue, NativeError> {
            let bytes = payload(ctx, data).map_err(|e| NativeError::std(StdErrorClass::Net, e))?;
            let task = driver().write(conn_id, bytes);
            Ok(ctx.task_from_host(task, HostOpen::Plain))
        }

        fn tcpClose(_ctx: &mut dyn NativeCtx, conn_id: i64) -> Result<(), NativeError> {
            driver().close(conn_id);
            Ok(())
        }

        fn tcpCloseListener(_ctx: &mut dyn NativeCtx, listener_id: i64) -> Result<(), NativeError> {
            driver().close_listener(listener_id);
            Ok(())
        }

        fn parseHttpRequest(ctx: &mut dyn NativeCtx, raw: &str) -> Result<Option<VmValue>, NativeError> {
            (http_parser::parse_http_request(ctx, raw)).map_err(|e| NativeError::std(StdErrorClass::Parse, e))
        }

        fn httpMethodName(ctx: &mut dyn NativeCtx, method: VmValue) -> Result<String, NativeError> {
            (http_parser::HttpMethod::from_value(ctx, method)
                .map(|m| m.name().to_owned())
                .ok_or_else(|| "expected an HttpMethod".to_owned())).map_err(|e| NativeError::std(StdErrorClass::Std, e))
        }

        fn sendHttpResponse(
            ctx: &mut dyn NativeCtx,
            conn_id: i64,
            status: i64,
            status_text: &str,
            headers: VmValue,
            body: &str,
            cookies: vn_types::VnArray,
        ) -> Result<VmValue, NativeError> {
            let mut out = http_head::encode_head(
                ctx,
                status,
                status_text,
                headers,
                cookies,
                http_head::Framing::Length(body.len()),
            )?;
            out.extend_from_slice(body.as_bytes());

            let task = driver().write(conn_id, out);
            Ok(ctx.task_from_host(task, HostOpen::Plain))
        }

        fn isHttpToken(_ctx: &mut dyn NativeCtx, name: &str) -> Result<bool, NativeError> {
            Ok(http_head::is_token(name))
        }

        fn isHttpFieldValue(_ctx: &mut dyn NativeCtx, value: &str) -> Result<bool, NativeError> {
            Ok(http_head::is_clean(value))
        }

        fn sendHttpHead(
            ctx: &mut dyn NativeCtx,
            conn_id: i64,
            status: i64,
            status_text: &str,
            headers: VmValue,
            cookies: vn_types::VnArray,
        ) -> Result<VmValue, NativeError> {
            let head = http_head::encode_head(
                ctx,
                status,
                status_text,
                headers,
                cookies,
                http_head::Framing::Chunked,
            )?;
            let task = driver().write(conn_id, head);
            Ok(ctx.task_from_host(task, HostOpen::Plain))
        }

        fn udpBind(ctx: &mut dyn NativeCtx, host: &str, port: i64) -> Result<i64, NativeError> {
            if !ctx.check_net_listen(port) {
                return Err(NativeError::capability("net.listen", port.to_string()));
            }
            driver().udp_bind(host, port).map_err(|e| NativeError::std(StdErrorClass::Net, e.to_string()).with_str(ErrorField::Host, host).with_int(ErrorField::Port, port))
        }

        fn udpSendTo(ctx: &mut dyn NativeCtx, socket_id: i64, host: &str, port: i64, data: VmValue) -> Result<i64, NativeError> {
            let bytes = payload(ctx, data).map_err(|e| NativeError::std(StdErrorClass::Net, e))?;
            let mut retries = 32;
            let sent = loop {
                match driver().udp_send_to(socket_id, &bytes, host, port) {
                    Ok(n) => break n,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock && retries > 0 => {
                        retries -= 1;
                        std::hint::spin_loop();
                    }
                    Err(e) => {
                        return Err(NativeError::std(StdErrorClass::Net, e.to_string())
                            .with_str(ErrorField::Host, host)
                            .with_int(ErrorField::Port, port));
                    }
                }
            };
            Ok(sent as i64)
        }

        fn udpRecvFrom(ctx: &mut dyn NativeCtx, socket_id: i64, max_len: i64) -> Result<VmValue, NativeError> {
            let task = driver().udp_recv(socket_id, max_len.max(0) as usize);
            Ok(ctx.task_from_host(task, HostOpen::Plain))
        }

        fn udpClose(_ctx: &mut dyn NativeCtx, socket_id: i64) -> Result<(), NativeError> {
            driver().udp_close(socket_id);
            Ok(())
        }
    }
}
