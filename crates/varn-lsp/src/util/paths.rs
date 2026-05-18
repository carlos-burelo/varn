pub fn uri_to_path_str(uri: &str) -> String {
    let s = uri
        .trim_start_matches("file:///")
        .trim_start_matches("file://");
    let s = s.replace("%20", " ");
    s.replace('\\', "/")
}

pub fn path_to_uri(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    format!("file:///{}", normalized.trim_start_matches('/'))
}

pub fn is_stdlib_uri(uri: &str) -> bool {
    uri.contains(crate::constants::STD_LIB_PATH_SEGMENT)
}

pub fn is_varn_file(path: &str) -> bool {
    path.ends_with(crate::constants::varn_EXTENSION)
}
