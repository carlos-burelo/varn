pub fn uri_to_path_str(uri: &str) -> String {
    varn_modules::resolver::uri_to_path(uri)
}

pub fn path_to_uri(path: &str) -> String {
    varn_modules::resolver::path_to_uri(path)
}

pub fn is_stdlib_uri(uri: &str) -> bool {
    crate::workspace::std_sources::is_mirrored_uri(uri)
}

pub fn is_varn_file(path: &str) -> bool {
    varn_modules::resolver::is_varn_file(path)
}
