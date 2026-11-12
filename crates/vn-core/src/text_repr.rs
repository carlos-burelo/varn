use std::fmt::Write;

pub fn write_quoted_str(s: &str, out: &mut String) {
    out.push('"');
    let bytes = s.as_bytes();

    if !bytes.iter().any(|&b| b == b'"' || b == b'\\' || b < 0x20) {
        out.push_str(s);
        out.push('"');
        return;
    }
    let mut start = 0;
    for (i, &b) in bytes.iter().enumerate() {
        let escaped = match b {
            b'"' => "\\\"",
            b'\\' => "\\\\",
            b'\x08' => "\\b",
            b'\x0c' => "\\f",
            b'\n' => "\\n",
            b'\r' => "\\r",
            b'\t' => "\\t",
            byte if byte < 0x20 => {
                if start < i {
                    out.push_str(&s[start..i]);
                }
                let _ = write!(out, "\\u{byte:04x}");
                start = i + 1;
                continue;
            }
            _ => continue,
        };
        if start < i {
            out.push_str(&s[start..i]);
        }
        out.push_str(escaped);
        start = i + 1;
    }
    if start < bytes.len() {
        out.push_str(&s[start..]);
    }
    out.push('"');
}
