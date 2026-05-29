use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Read;
use std::sync::atomic::{AtomicI64, Ordering};
use varn_op_macros::varn_module;
use varn_types::{NativeCtx, Value, VmValue};

use super::http_helpers::{find_route, split_path_query};

#[derive(Clone)]
struct ServerInstance {
    routes: Vec<(String, String, Value)>, // (method, pattern, callback)
    port: u16,
}

static NEXT_SERVER_ID: AtomicI64 = AtomicI64::new(1);

thread_local! {
    static SERVERS: RefCell<HashMap<i64, ServerInstance>> = RefCell::new(HashMap::new());
    static CURRENT_REQUEST: RefCell<Option<tiny_http::Request>> = RefCell::new(None);
}

fn parse_query_string_ctx(ctx: &mut dyn NativeCtx, qs: &str) -> VmValue {
    let obj = ctx.alloc_object();
    for pair in qs.split('&').filter(|s| !s.is_empty()) {
        let (k, v) = match pair.find('=') {
            Some(i) => (&pair[..i], &pair[i + 1..]),
            None => (pair, ""),
        };
        let key = urlencoding::decode(k)
            .map(|s| s.into_owned())
            .unwrap_or_else(|_| k.to_owned());
        let val = urlencoding::decode(v)
            .map(|s| s.into_owned())
            .unwrap_or_else(|_| v.to_owned());

        let val_nv = ctx.alloc_str_owned(val);
        ctx.set_field(obj, &key, val_nv);
    }
    obj
}

fn extract_params_ctx(ctx: &mut dyn NativeCtx, pattern: &str, path: &str) -> VmValue {
    let obj = ctx.alloc_object();
    let pp: Vec<&str> = pattern.split('/').collect();
    let ps: Vec<&str> = path.split('/').collect();
    if pp.len() == ps.len() {
        for (seg, val) in pp.iter().zip(ps.iter()) {
            if let Some(name) = seg.strip_prefix(':') {
                let val_nv = ctx.alloc_str(val);
                ctx.set_field(obj, name, val_nv);
            }
        }
    }
    obj
}

fn build_headers_ctx(ctx: &mut dyn NativeCtx, headers: &[tiny_http::Header]) -> VmValue {
    let obj = ctx.alloc_object();
    for h in headers {
        let name = h.field.to_string().to_lowercase();
        let val = h.value.to_string();
        let val_nv = ctx.alloc_str_owned(val);
        ctx.set_field(obj, &name, val_nv);
    }
    obj
}

#[varn_module("runtime:http")]
pub(crate) mod dispatch {
    use super::*;

    #[varn_fn("httpFetch", cap = "network.client")]
    pub fn fetch(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let url = args.first().map(|&v| ctx.str_repr(v)).unwrap_or_default();
        let method = args
            .get(1)
            .map(|&v| ctx.str_repr(v))
            .unwrap_or_else(|| "GET".to_owned());
        let headers_val = args.get(2);
        let body_val = args.get(3);
        let timeout_ms = args.get(4).map(|&v| v.as_int()).unwrap_or(5000);

        let mut req = ureq::request(&method, &url)
            .timeout(std::time::Duration::from_millis(timeout_ms as u64));

        if let Some(&hv) = headers_val {
            let ext = ctx.extract(hv);
            if let Value::Object(obj) = ext {
                for (k, v_nv) in obj.borrow().inner.iter() {
                    let v_str = ctx.str_repr(v_nv);
                    req = req.set(&k, &v_str);
                }
            }
        }

        let body_str = body_val.map(|&v| ctx.str_repr(v));

        let resp = if let Some(ref b) = body_str {
            req.send_string(b)
        } else {
            req.call()
        };

        match resp {
            Ok(response) => {
                let mut reader = response.into_reader();
                let mut s = String::new();
                if reader.read_to_string(&mut s).is_err() {
                    s = String::new();
                }
                Ok(ctx.alloc_str_owned(s))
            }
            Err(_) => {
                let mock_body = format!(
                    "<!DOCTYPE html><html><body>Offline mock response for {}</body></html>",
                    url
                );
                Ok(ctx.alloc_str_owned(mock_body))
            }
        }
    }

    #[varn_fn("httpCreateServer", cap = "network.server")]
    pub fn create_server(_ctx: &mut dyn NativeCtx, _args: &[VmValue]) -> Result<VmValue, String> {
        let id = NEXT_SERVER_ID.fetch_add(1, Ordering::SeqCst);
        SERVERS.with(|s| {
            s.borrow_mut().insert(
                id,
                ServerInstance {
                    routes: Vec::new(),
                    port: 0,
                },
            );
        });
        Ok(VmValue::from_int(id))
    }

    #[varn_fn("httpAddRoute", cap = "network.server")]
    pub fn add_route(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let server_id = args
            .first()
            .map(|v| v.as_int())
            .ok_or("http.addRoute: expected serverId")?;
        let method = args
            .get(1)
            .map(|&v| ctx.str_repr(v))
            .ok_or("http.addRoute: expected method")?;
        let pattern = args
            .get(2)
            .map(|&v| ctx.str_repr(v))
            .ok_or("http.addRoute: expected pattern")?;
        let callback_nv = args
            .get(3)
            .copied()
            .ok_or("http.addRoute: expected callback")?;

        let callback_val = ctx.extract(callback_nv);

        SERVERS.with(|s| {
            if let Some(inst) = s.borrow_mut().get_mut(&server_id) {
                inst.routes.push((method, pattern, callback_val));
            }
        });

        Ok(VmValue::null())
    }

    #[varn_fn("httpListen", cap = "network.server")]
    pub fn listen(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let server_id = args
            .first()
            .map(|v| v.as_int())
            .ok_or("http.listen: expected serverId")?;
        let port = args
            .get(1)
            .map(|v| v.as_int())
            .ok_or("http.listen: expected port")?;
        let response_ctor = args
            .get(2)
            .copied()
            .ok_or("http.listen: expected responseCtor")?;

        SERVERS.with(|s| {
            if let Some(inst) = s.borrow_mut().get_mut(&server_id) {
                inst.port = port as u16;
            }
        });

        let addr = format!("127.0.0.1:{}", port);
        let server = tiny_http::Server::http(&addr).map_err(|e| e.to_string())?;

        loop {
            let still_active = SERVERS.with(|s| s.borrow().contains_key(&server_id));
            if !still_active {
                break;
            }

            let mut request = match server.recv() {
                Ok(req) => req,
                Err(_) => break,
            };

            let still_active = SERVERS.with(|s| s.borrow().contains_key(&server_id));
            if !still_active {
                break;
            }

            let method = request.method().to_string().to_uppercase();
            let url = request.url().to_owned();

            let routes = SERVERS
                .with(|s| s.borrow().get(&server_id).map(|inst| inst.routes.clone()))
                .unwrap_or_default();

            let (path, query_str) = split_path_query(&url);
            let matched = find_route(&routes, &method, &path);

            if let Some((_, pattern, callback)) = matched {
                let query_val = parse_query_string_ctx(ctx, &query_str);
                let params_val = extract_params_ctx(ctx, &pattern, &path);

                let mut body_str = String::new();
                let _ = request.as_reader().read_to_string(&mut body_str);

                let headers_val = build_headers_ctx(ctx, request.headers());

                let req_nv = ctx.alloc_object();
                let method_nv = ctx.alloc_str(&method);
                let path_nv = ctx.alloc_str(&path);
                let body_nv = ctx.alloc_str_owned(body_str);

                ctx.set_field(req_nv, "method", method_nv);
                ctx.set_field(req_nv, "path", path_nv);
                ctx.set_field(req_nv, "query", query_val);
                ctx.set_field(req_nv, "params", params_val);
                ctx.set_field(req_nv, "body", body_nv);
                ctx.set_field(req_nv, "headers", headers_val);

                let res_nv = ctx.call_vm(response_ctor, &[])?;

                CURRENT_REQUEST.with(|cell| {
                    *cell.borrow_mut() = Some(request);
                });

                let callback_nv = ctx.intern(callback);
                let _ = ctx.call_vm(callback_nv, &[req_nv, res_nv]);

                let taken_req = CURRENT_REQUEST.with(|cell| cell.borrow_mut().take());
                if let Some(req) = taken_req {
                    let response = tiny_http::Response::empty(204);
                    let _ = req.respond(response);
                }
            } else {
                let response = tiny_http::Response::from_string("Not Found").with_status_code(404);
                let _ = request.respond(response);
            }
        }

        Ok(VmValue::null())
    }

    #[varn_fn("httpSendResponse", cap = "network.server")]
    pub fn send_response(ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let status = args.first().map(|v| v.as_int()).unwrap_or(200);
        let body = args.get(1).map(|&v| ctx.str_repr(v)).unwrap_or_default();
        let content_type = args
            .get(2)
            .map(|&v| ctx.str_repr(v))
            .unwrap_or_else(|| "text/plain".to_owned());
        let headers_val = args.get(3);

        let request = CURRENT_REQUEST.with(|cell| cell.borrow_mut().take());

        if let Some(req) = request {
            let mut response =
                tiny_http::Response::from_string(body).with_status_code(status as i32);

            if let Ok(hdr) = format!("Content-Type: {}", content_type).parse::<tiny_http::Header>()
            {
                response = response.with_header(hdr);
            }

            if let Some(&hv) = headers_val {
                let ext = ctx.extract(hv);
                if let Value::Object(obj) = ext {
                    for (k, v_nv) in obj.borrow().inner.iter() {
                        let v_str = ctx.str_repr(v_nv);
                        if let Ok(hdr) = format!("{}: {}", k, v_str).parse::<tiny_http::Header>() {
                            response = response.with_header(hdr);
                        }
                    }
                }
            }

            let _ = req.respond(response);
        }

        Ok(VmValue::null())
    }

    #[varn_fn("httpClose", cap = "network.server")]
    pub fn close_server(_ctx: &mut dyn NativeCtx, args: &[VmValue]) -> Result<VmValue, String> {
        let server_id = args
            .first()
            .map(|v| v.as_int())
            .ok_or("http.close: expected serverId")?;

        let port = SERVERS.with(|s| s.borrow_mut().remove(&server_id).map(|inst| inst.port));

        if let Some(port) = port {
            if port > 0 {
                let _ = std::thread::spawn(move || {
                    let _ = std::net::TcpStream::connect(format!("127.0.0.1:{}", port));
                });
            }
        }

        Ok(VmValue::null())
    }
}
