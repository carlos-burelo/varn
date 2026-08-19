pub mod driver;

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

        fn tcpListen(_ctx: &mut dyn NativeCtx, port: i64) -> Result<i64, String> {
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
    }
}
