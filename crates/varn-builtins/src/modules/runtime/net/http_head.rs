use std::io::Write;
use varn_types::{NativeCtx, NativeError, VmValue, VnArray};

const FRAMING_HEADERS: [&str; 2] = ["content-length", "transfer-encoding"];
const TOKEN_PUNCTUATION: &[u8] = b"!#$%&'*+-.^_`|~";

pub(super) enum Framing {
    Length(usize),
    Chunked,
}

pub(super) fn is_token(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || TOKEN_PUNCTUATION.contains(&b))
}

pub(super) fn is_clean(text: &str) -> bool {
    !text.bytes().any(|b| b == b'\r' || b == b'\n' || b == 0)
}

fn rejected(what: &str, shown: &str) -> NativeError {
    NativeError::type_error(format!("invalid HTTP {what} '{}'", shown.escape_debug()))
}

pub(super) fn encode_head(
    ctx: &mut dyn NativeCtx,
    status: i64,
    status_text: &str,
    headers: VmValue,
    cookies: VnArray,
    framing: Framing,
) -> Result<Vec<u8>, NativeError> {
    if !(100..=999).contains(&status) {
        return Err(NativeError::range_error(format!(
            "an HTTP status must have three digits, got {status}"
        )));
    }
    if !is_clean(status_text) {
        return Err(rejected("status text", status_text));
    }
    let mut fields: Vec<(String, String)> = Vec::new();
    if !headers.is_null() {
        let mut collected: Vec<(VmValue, VmValue)> = Vec::new();
        ctx.map_for_each(headers, &mut |key, value| collected.push((key, value)));
        for (key, value) in collected {
            let (Some(name), Some(text)) = (ctx.str_owned(key), ctx.str_owned(value)) else {
                continue;
            };
            if !is_token(&name) {
                return Err(rejected("header name", &name));
            }
            if !is_clean(&text) {
                return Err(rejected("header value", &text));
            }
            fields.push((name, text));
        }
    }
    let mut out = Vec::with_capacity(160);
    let _ = write!(&mut out, "HTTP/1.1 {status} {status_text}\r\n");
    let owns_framing = |name: &str| {
        FRAMING_HEADERS
            .iter()
            .any(|framing| name.eq_ignore_ascii_case(framing))
    };
    let mut has_content_type = false;
    let mut has_content_length = false;
    for (name, text) in &fields {
        if matches!(framing, Framing::Chunked) && owns_framing(name) {
            continue;
        }
        if name.eq_ignore_ascii_case("content-type") {
            has_content_type = true;
        }
        if name.eq_ignore_ascii_case("content-length") {
            if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
                return Err(rejected("Content-Length", text));
            }
            has_content_length = true;
        }
        if name.eq_ignore_ascii_case("transfer-encoding") {
            continue;
        }
        let _ = write!(&mut out, "{name}: {text}\r\n");
    }
    if !has_content_type {
        let _ = write!(&mut out, "Content-Type: text/plain\r\n");
    }
    match framing {
        Framing::Length(length) if !has_content_length => {
            let _ = write!(&mut out, "Content-Length: {length}\r\n");
        }
        Framing::Length(_) => {}
        Framing::Chunked => {
            let _ = write!(&mut out, "Transfer-Encoding: chunked\r\n");
        }
    }
    for index in 0..cookies.len(ctx) {
        let Some(cookie) = cookies.get(ctx, index).and_then(|v| ctx.str_owned(v)) else {
            continue;
        };
        if !is_clean(&cookie) {
            return Err(rejected("cookie", &cookie));
        }
        let _ = write!(&mut out, "Set-Cookie: {cookie}\r\n");
    }
    out.extend_from_slice(b"\r\n");
    Ok(out)
}
