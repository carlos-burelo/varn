//! Stable op-ids for native operations.
//!
//! An op-id is a build-stable FNV-1a hash over the operation's identity
//! (`module::symbol`, or `module::class::symbol` for class members). It is the
//! single identity shared by the compiler (which embeds it in bytecode) and the
//! runtime dispatch table (which looks it up). Because it is a pure function of
//! fixed strings it is identical across builds and platforms, so it is safe to
//! serialize into cached `.vnc` bytecode.
//!
//! This lives in `varn-core` so both the `varn-compiler` crate and the
//! `varn-builtins` runtime compute the exact same id (neither can depend on the
//! other).

use crate::type_tag::TypeTag;

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

/// The core builtin classes whose instance methods are natively registered
/// (via the `varn_contract!` invocations in `varn-builtins/src/modules/
/// primitives/*`) and are therefore op-id-addressable.
///
/// This is the **single source of truth** for "is this a core type with a
/// native method table". Adding a primitive = add its [`TypeTag`] here and
/// nothing else in the dispatch layer. Distinct from the VM's intrinsic-class
/// registry (which also carries `Error`/`TypeError`/`RangeError` for property
/// fallback) — these are the op-id-dispatched primitives.
pub const CORE_CLASSES: [TypeTag; 12] = [
    TypeTag::Array,
    TypeTag::Str,
    TypeTag::Map,
    TypeTag::Set,
    TypeTag::Range,
    TypeTag::Symbol,
    TypeTag::Int,
    TypeTag::Float,
    TypeTag::Bool,
    TypeTag::Char,
    TypeTag::Decimal,
    TypeTag::BigInt,
];

/// Whether `tag` is a core class with a natively registered method table.
#[inline]
pub fn is_core_class(tag: TypeTag) -> bool {
    CORE_CLASSES.contains(&tag)
}

/// The class-registration name for a core class, i.e. the exact string used as
/// the `class:` key in `varn_contract!` and in the `.vn` contracts. This is the
/// string that feeds [`core_method_op_id`], so it must match the registration
/// byte-for-byte — and it is the single canonical [`TypeTag::name`].
pub fn core_class_name(tag: TypeTag) -> Option<&'static str> {
    is_core_class(tag).then(|| tag.name())
}

/// Exact inverse of [`core_class_name`]: a registration name back to its
/// [`TypeTag`], restricted to core classes. Only the canonical registration
/// name resolves (e.g. `"Symbol"`, not the surface `"symbol"`), so this never
/// admits a name that wouldn't round-trip through [`core_class_name`]. Returns
/// `None` for user classes / unknown names.
pub fn core_class_tag(name: &str) -> Option<TypeTag> {
    CORE_CLASSES
        .into_iter()
        .find(|&tag| core_class_name(tag) == Some(name))
}

/// Maps a core class name to `Some(class)` when it is one whose methods are
/// natively registered (and therefore op-id-addressable). Returns `None` for
/// user classes / unknown receivers, so the compiler only emits a direct
/// `CallNativeOp` when the dispatch is guaranteed to resolve.
pub fn core_class(name: &str) -> Option<&'static str> {
    core_class_tag(name).and_then(core_class_name)
}
