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

        fn tcpWrite(ctx: &mut dyn NativeCtx, conn_id: i64, data: VmValue) -> Result<VmValue, String> {
            let bytes = if ctx.is_buffer(data) {
                ctx.buffer_to_bytes(data).unwrap_or_default()
            } else if let Some(s) = ctx.str_owned(data) {
                s.into_bytes()
            } else {
                ctx.str_repr(data).into_bytes()
            };
            let task = driver().write(conn_id, bytes);
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

        fn udpBind(ctx: &mut dyn NativeCtx, host: &str, port: i64) -> Result<i64, String> {
            if !ctx.check_net_listen(port) {
                return Err(format!("SecurityError: Permission denied (net.listen) on port {port}"));
            }
            let addr = format!("{host}:{port}");
            let socket = std::net::UdpSocket::bind(&addr).map_err(|e| format!("udpBind error: {e}"))?;
            let id = NEXT_UDP_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            UDP_SOCKETS.write().unwrap().insert(id, std::sync::Arc::new(socket));
            Ok(id)
        }

        fn udpSendTo(ctx: &mut dyn NativeCtx, socket_id: i64, host: &str, port: i64, data: VmValue) -> Result<i64, String> {
            let sock = {
                let map = UDP_SOCKETS.read().unwrap();
                map.get(&socket_id).cloned().ok_or_else(|| format!("invalid UDP socket id {socket_id}"))?
            };
            let bytes = if ctx.is_buffer(data) {
                ctx.buffer_to_bytes(data).unwrap_or_default()
            } else if let Some(s) = ctx.str_owned(data) {
                s.into_bytes()
            } else {
                ctx.str_repr(data).into_bytes()
            };
            let addr = format!("{host}:{port}");
            let sent = sock.send_to(&bytes, &addr).map_err(|e| format!("udpSendTo error: {e}"))?;
            Ok(sent as i64)
        }

        fn udpRecvFrom(ctx: &mut dyn NativeCtx, socket_id: i64, max_len: i64) -> Result<VmValue, String> {
            let sock = {
                let map = UDP_SOCKETS.read().unwrap();
                map.get(&socket_id).cloned().ok_or_else(|| format!("invalid UDP socket id {socket_id}"))?
            };
            let task = varn_types::AsyncTask::pending();
            let task_clone = task.clone();
            std::thread::spawn(move || {
                let mut buf = vec![0u8; max_len.max(64) as usize];
                match sock.recv_from(&mut buf) {
                    Ok((amt, src_addr)) => {
                        buf.truncate(amt);
                        let host_rc = std::rc::Rc::from(src_addr.ip().to_string().as_str());
                        let packet_val = varn_types::value::new_array(vec![
                            Value::Buffer(varn_types::VmBuffer::from_bytes(&buf)),
                            Value::Str(host_rc),
                            Value::Int(src_addr.port() as i64),
                        ]);
                        task_clone.settle(Ok(packet_val));
                    }
                    Err(_) => {
                        task_clone.settle(Ok(Value::Null));
                    }
                }
            });
            Ok(ctx.intern(Value::TaskHandle(task)))
        }

        fn udpClose(_ctx: &mut dyn NativeCtx, socket_id: i64) -> Result<(), String> {
            UDP_SOCKETS.write().unwrap().remove(&socket_id);
            Ok(())
        }
    }
}

static NEXT_UDP_ID: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(1);
static UDP_SOCKETS: std::sync::LazyLock<std::sync::RwLock<std::collections::HashMap<i64, std::sync::Arc<std::net::UdpSocket>>>> =
    std::sync::LazyLock::new(|| std::sync::RwLock::new(std::collections::HashMap::new()));
