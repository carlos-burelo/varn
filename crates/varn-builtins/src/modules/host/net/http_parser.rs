use varn_types::{NativeCtx, VmValue};

pub fn parse_http_request(ctx: &mut dyn NativeCtx, raw: &str) -> Result<VmValue, String> {
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut req = httparse::Request::new(&mut headers);
    let bytes = raw.as_bytes();

    let status = match req.parse(bytes) {
        Ok(s) => s,
        Err(_) => return Ok(ctx.null_val()),
    };

    let header_len = match status {
        httparse::Status::Complete(len) => len,
        httparse::Status::Partial => return Ok(ctx.null_val()),
    };

    let method = req.method.unwrap_or("");
    let path = req.path.unwrap_or("");
    let body = if header_len < raw.len() {
        &raw[header_len..]
    } else {
        ""
    };

    // 1. Build the headers object in the VM
    let headers_obj = ctx.alloc_object();
    for header in req.headers.iter() {
        let name = header.name.to_lowercase();
        let value = String::from_utf8_lossy(header.value);
        let val_nv = ctx.alloc_str(value.as_ref());
        ctx.set_field(headers_obj, &name, val_nv);
    }

    // 2. Build the result RawRequest object
    let result_obj = ctx.alloc_object();
    let method_nv = ctx.alloc_str(method);
    let path_nv = ctx.alloc_str(path);
    let body_nv = ctx.alloc_str(body);

    ctx.set_field(result_obj, "method", method_nv);
    ctx.set_field(result_obj, "path", path_nv);
    ctx.set_field(result_obj, "headers", headers_obj);
    ctx.set_field(result_obj, "body", body_nv);

    Ok(result_obj)
}
