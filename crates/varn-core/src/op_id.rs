












use crate::runtime_kind::RuntimeKind;

#[inline]
fn fnv1a(segments: &[&[u8]]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for seg in segments {
        for &b in *seg {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    h
}


pub fn compound_op_id(module_id: &str, symbol: &str) -> u64 {
    fnv1a(&[module_id.as_bytes(), b"::", symbol.as_bytes()])
}




pub fn compound_op_id3(module_id: &str, class: &str, symbol: &str) -> u64 {
    fnv1a(&[
        module_id.as_bytes(),
        b"::",
        class.as_bytes(),
        b"::",
        symbol.as_bytes(),
    ])
}




pub const CORE_MODULE: &str = "globals";


pub fn core_method_op_id(class: &str, method: &str) -> u64 {
    compound_op_id3(CORE_MODULE, class, method)
}









pub fn array_push_op_id() -> u64 {
    static ID: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *ID.get_or_init(|| core_method_op_id(crate::RuntimeKind::Array.name(), "push"))
}

pub fn str_starts_with_op_id() -> u64 {
    static ID: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *ID.get_or_init(|| core_method_op_id(crate::RuntimeKind::Str.name(), "startsWith"))
}

pub fn str_ends_with_op_id() -> u64 {
    static ID: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *ID.get_or_init(|| core_method_op_id(crate::RuntimeKind::Str.name(), "endsWith"))
}

pub fn str_slice_op_id() -> u64 {
    static ID: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *ID.get_or_init(|| core_method_op_id(crate::RuntimeKind::Str.name(), "slice"))
}

pub fn str_index_of_op_id() -> u64 {
    static ID: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *ID.get_or_init(|| core_method_op_id(crate::RuntimeKind::Str.name(), "indexOf"))
}

pub fn str_includes_op_id() -> u64 {
    static ID: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *ID.get_or_init(|| core_method_op_id(crate::RuntimeKind::Str.name(), "includes"))
}

pub fn str_split_op_id() -> u64 {
    static ID: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *ID.get_or_init(|| core_method_op_id(crate::RuntimeKind::Str.name(), "split"))
}




pub fn str_char_code_at_op_id() -> u64 {
    static ID: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *ID.get_or_init(|| core_method_op_id(crate::RuntimeKind::Str.name(), "charCodeAt"))
}

pub fn str_code_point_at_op_id() -> u64 {
    static ID: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *ID.get_or_init(|| core_method_op_id(crate::RuntimeKind::Str.name(), "codePointAt"))
}


pub fn is_str_char_index_op_id(op_id: u64) -> bool {
    op_id == str_char_code_at_op_id() || op_id == str_code_point_at_op_id()
}










pub const CORE_CLASSES: [RuntimeKind; 13] = [
    RuntimeKind::Array,
    RuntimeKind::Str,
    RuntimeKind::Bytes,
    RuntimeKind::Map,
    RuntimeKind::Set,
    RuntimeKind::Range,
    RuntimeKind::Symbol,
    RuntimeKind::Int,
    RuntimeKind::Float,
    RuntimeKind::Bool,
    RuntimeKind::Char,
    RuntimeKind::Decimal,
    RuntimeKind::BigInt,
];


#[inline]
pub fn is_core_class(tag: RuntimeKind) -> bool {
    CORE_CLASSES.contains(&tag)
}





pub fn core_class_name(tag: RuntimeKind) -> Option<&'static str> {
    is_core_class(tag).then(|| tag.name())
}






pub fn core_class_tag(name: &str) -> Option<RuntimeKind> {
    CORE_CLASSES
        .into_iter()
        .find(|&tag| core_class_name(tag) == Some(name))
}





pub fn core_class(name: &str) -> Option<&'static str> {
    core_class_tag(name).and_then(core_class_name)
}
