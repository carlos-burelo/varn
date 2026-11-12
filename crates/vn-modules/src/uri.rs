use super::normalize_path_string;

pub fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(h), Some(l)) = (hi, lo) {
                out.push((h * 16 + l) as u8 as char);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

pub fn uri_to_path(uri: &str) -> String {
    let decoded = percent_decode(uri);
    let uri = decoded.as_str();
    if let Some(rest) = uri.strip_prefix("file:///") {
        #[cfg(windows)]
        return normalize_path_string(rest.replace('/', "\\"));
        #[cfg(not(windows))]
        return normalize_path_string(format!("/{rest}"));
    }
    if let Some(rest) = uri.strip_prefix("file://") {
        return normalize_path_string(rest.to_owned());
    }
    uri.to_owned()
}

pub fn path_to_uri(path: &str) -> String {
    #[cfg(windows)]
    {
        let normalized = path.replace('\\', "/");
        format!("file:///{normalized}")
    }
    #[cfg(not(windows))]
    {
        format!("file://{path}")
    }
}
