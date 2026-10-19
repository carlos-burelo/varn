use varn_types::{NativeCtx, VmValue};

varn_macros::varn_record! {
    contract: "src/modules/runtime/net/net_runtime.vn",
    interface: "HttpRequestHead",
    struct HttpRequestHead;
}

pub fn parse_http_request(ctx: &mut dyn NativeCtx, raw: &str) -> Result<Option<VmValue>, String> {
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut req = httparse::Request::new(&mut headers);
    let bytes = raw.as_bytes();

    let status = match req.parse(bytes) {
        Ok(s) => s,
        Err(_) => return Ok(None),
    };

    let header_len = match status {
        httparse::Status::Complete(len) => len,
        httparse::Status::Partial => return Ok(None),
    };

    let method = req.method.unwrap_or("");
    let path = req.path.unwrap_or("");
    let body = if header_len < raw.len() {
        &raw[header_len..]
    } else {
        ""
    };

    let headers_obj = ctx.alloc_object();
    for header in req.headers.iter() {
        let name = header.name.to_lowercase();
        let value = String::from_utf8_lossy(header.value);
        let val_nv = ctx.alloc_str(value.as_ref());
        ctx.set_field(headers_obj, &name, val_nv);
    }

    let method = ctx.alloc_str(method);
    let path = ctx.alloc_str(path);
    let body = ctx.alloc_str(body);
    Ok(Some(
        HttpRequestHead {
            method,
            path,
            headers: headers_obj,
            body,
        }
        .alloc(ctx),
    ))
}
