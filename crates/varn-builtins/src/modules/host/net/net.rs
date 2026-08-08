use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use urlencoding::{decode, encode};
use varn_op_macros::varn_contract;
use varn_types::{NativeCtx, Value, VmValue};

static NEXT_SOCKET_ID: AtomicI64 = AtomicI64::new(1);

fn listeners() -> &'static Mutex<HashMap<i64, Arc<std::net::TcpListener>>> {
    static INSTANCE: OnceLock<Mutex<HashMap<i64, Arc<std::net::TcpListener>>>> = OnceLock::new();
    INSTANCE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn streams() -> &'static Mutex<HashMap<i64, Arc<Mutex<std::net::TcpStream>>>> {
    static INSTANCE: OnceLock<Mutex<HashMap<i64, Arc<Mutex<std::net::TcpStream>>>>> =
        OnceLock::new();
    INSTANCE.get_or_init(|| Mutex::new(HashMap::new()))
}

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
            let listener = match std::net::TcpListener::bind(format!("127.0.0.1:{port}")) {
                Ok(l) => l,
                Err(_) => return Ok(-1),
            };
            let id = NEXT_SOCKET_ID.fetch_add(1, Ordering::SeqCst);
            listeners().lock().unwrap().insert(id, Arc::new(listener));
            Ok(id)
        }

        fn tcpAccept(ctx: &mut dyn NativeCtx, listener_id: i64) -> Result<VmValue, String> {
            let listener = match listeners().lock().unwrap().get(&listener_id).cloned() {
                Some(l) => l,
                None => {
                    let task = varn_types::AsyncTask::pending();
                    task.settle(Ok(Value::Int(-1)));
                    return Ok(ctx.intern(Value::TaskHandle(task)));
                }
            };

            let task = varn_types::AsyncTask::pending();
            let task_clone = task.clone();

            std::thread::spawn(move || {
                match listener.accept() {
                    Ok((stream, _addr)) => {
                        let conn_id = NEXT_SOCKET_ID.fetch_add(1, Ordering::SeqCst);
                        streams().lock().unwrap().insert(conn_id, Arc::new(Mutex::new(stream)));
                        task_clone.settle(Ok(Value::Int(conn_id)));
                    }
                    Err(_) => {
                        task_clone.settle(Ok(Value::Int(-1)));
                    }
                }
            });

            Ok(ctx.intern(Value::TaskHandle(task)))
        }

        fn tcpConnect(ctx: &mut dyn NativeCtx, host: &str, port: i64) -> Result<VmValue, String> {
            let addr = format!("{host}:{port}");
            let task = varn_types::AsyncTask::pending();
            let task_clone = task.clone();

            std::thread::spawn(move || {
                match std::net::TcpStream::connect(&addr) {
                    Ok(stream) => {
                        let conn_id = NEXT_SOCKET_ID.fetch_add(1, Ordering::SeqCst);
                        streams().lock().unwrap().insert(conn_id, Arc::new(Mutex::new(stream)));
                        task_clone.settle(Ok(Value::Int(conn_id)));
                    }
                    Err(_) => {
                        task_clone.settle(Ok(Value::Int(-1)));
                    }
                }
            });

            Ok(ctx.intern(Value::TaskHandle(task)))
        }

        fn tcpRead(ctx: &mut dyn NativeCtx, conn_id: i64, len: i64) -> Result<VmValue, String> {
            let stream = match streams().lock().unwrap().get(&conn_id).cloned() {
                Some(s) => s,
                None => {
                    let task = varn_types::AsyncTask::pending();
                    task.settle(Ok(Value::Str(std::rc::Rc::from(""))));
                    return Ok(ctx.intern(Value::TaskHandle(task)));
                }
            };

            let task = varn_types::AsyncTask::pending();
            let task_clone = task.clone();

            std::thread::spawn(move || {
                let mut buf = vec![0u8; len as usize];
                let mut guard = stream.lock().unwrap();
                match guard.read(&mut buf) {
                    Ok(n) => {
                        buf.truncate(n);
                        let s = String::from_utf8_lossy(&buf).into_owned();
                        task_clone.settle(Ok(Value::Str(std::rc::Rc::from(s.as_str()))));
                    }
                    Err(_) => {
                        task_clone.settle(Ok(Value::Str(std::rc::Rc::from(""))));
                    }
                }
            });

            Ok(ctx.intern(Value::TaskHandle(task)))
        }

        fn tcpWrite(ctx: &mut dyn NativeCtx, conn_id: i64, data: &str) -> Result<VmValue, String> {
            let stream = match streams().lock().unwrap().get(&conn_id).cloned() {
                Some(s) => s,
                None => {
                    let task = varn_types::AsyncTask::pending();
                    task.settle(Ok(Value::Int(-1)));
                    return Ok(ctx.intern(Value::TaskHandle(task)));
                }
            };

            let data_vec = data.as_bytes().to_vec();
            let task = varn_types::AsyncTask::pending();
            let task_clone = task.clone();

            std::thread::spawn(move || {
                let mut guard = stream.lock().unwrap();
                match guard.write_all(&data_vec) {
                    Ok(()) => {
                        task_clone.settle(Ok(Value::Int(data_vec.len() as i64)));
                    }
                    Err(_) => {
                        task_clone.settle(Ok(Value::Int(-1)));
                    }
                }
            });

            Ok(ctx.intern(Value::TaskHandle(task)))
        }

        fn tcpClose(_ctx: &mut dyn NativeCtx, conn_id: i64) -> Result<(), String> {
            streams().lock().unwrap().remove(&conn_id);
            Ok(())
        }

        fn tcpCloseListener(_ctx: &mut dyn NativeCtx, listener_id: i64) -> Result<(), String> {
            listeners().lock().unwrap().remove(&listener_id);
            Ok(())
        }
    }
}
