//! Stable op-ids for native operations.
//!
//! An op-id is a build-stable FNV-1a hash over the operation's identity
//! (`module::symbol`, or `module::class::symbol` for class members). It is the
//! single identity shared by the compiler (which embeds it in bytecode) and the
//! runtime dispatch table (which looks it up). Because it is a pure function of
//! fixed strings it is identical across builds and platforms, so it is safe to
//! serialize into cached `.vnc` bytecode.
//!
//! This lives in `varn-core` so both the `varn-opt` compiler and the
//! `varn-builtins` runtime compute the exact same id (neither can depend on the
//! other).

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

/// Stable op-id for a module-level symbol: `module::symbol`.
pub fn compound_op_id(module_id: &str, symbol: &str) -> u64 {
    fnv1a(&[module_id.as_bytes(), b"::", symbol.as_bytes()])
}

/// Stable op-id for a class-qualified member: `module::class::symbol`.
/// The extra `::class` segment guarantees these ids never collide with the
/// 2-segment [`compound_op_id`] space.
pub fn compound_op_id3(module_id: &str, class: &str, symbol: &str) -> u64 {
    fnv1a(&[
        module_id.as_bytes(),
        b"::",
        class.as_bytes(),
        b"::",
        symbol.as_bytes(),
    ])
}

/// The native module under which all core-type classes (Array, str, int, Map,
/// …) are registered. See the `varn_contract!` invocations in
/// `varn-builtins/src/modules/primitives/*`.
pub const CORE_MODULE: &str = "globals";

/// op-id for a core-type method/getter, given its class name and member name.
pub fn core_method_op_id(class: &str, method: &str) -> u64 {
    compound_op_id3(CORE_MODULE, class, method)
}

/// Maps a core class name to `Some(class)` when it is one whose methods are
/// natively registered (and therefore op-id-addressable). Returns `None` for
/// user classes / unknown receivers, so the compiler only emits a direct
/// `CallNativeOp` when the dispatch is guaranteed to resolve.
pub fn core_class(name: &str) -> Option<&'static str> {
    match name {
        "Array" => Some("Array"),
        "str" => Some("str"),
        "Map" => Some("Map"),
        "Set" => Some("Set"),
        "Range" => Some("Range"),
        "Symbol" => Some("Symbol"),
        "int" => Some("int"),
        "float" => Some("float"),
        "bool" => Some("bool"),
        "char" => Some("char"),
        "decimal" => Some("decimal"),
        "bigint" => Some("bigint"),
        _ => None,
    }
}
