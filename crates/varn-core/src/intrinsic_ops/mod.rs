pub mod collections;
pub mod int;
pub mod map;
pub mod math;
pub mod str;
pub mod wire;

pub use map::lookup as intrinsic_lookup;
pub use wire::{decode as intrinsic_decode, encode as intrinsic_encode, IntrinsicDomain};

/// Whether the intrinsic named by `wire_byte` can allocate on the VM heap.
///
/// The JIT asks this to decide whether a loop body is allocation-free, which
/// is what licenses hoisting a resolved heap pointer out of it: an allocation
/// can grow the nursery or old-generation slot `Vec` and move every slot with
/// it, so a hoisted pointer would dangle. Being wrong in the `false` direction
/// is a use-after-move, so anything not PROVEN allocation-free answers `true`.
///
/// Only the `Str` domain is classified. Its non-allocating members are exactly
/// the ones whose result is an `int` or a `bool` — they read the receiver's
/// bytes and return a scalar. The ops that build a new string (`Substring`,
/// `Slice`, `Substr`, `At`) allocate, and every other domain stays
/// conservative until its implementation has been read with this question in
/// mind.
pub fn intrinsic_allocates(wire_byte: u8) -> bool {
    let (domain, op) = wire::decode(wire_byte);
    if domain != IntrinsicDomain::Str as u8 {
        return true;
    }
    !matches!(
        op,
        o if o == self::str::StrOp::CharCodeAt as u8
            || o == self::str::StrOp::CodePointAt as u8
            || o == self::str::StrOp::CharCode as u8
            || o == self::str::StrOp::IndexOf as u8
            || o == self::str::StrOp::LastIndexOf as u8
            || o == self::str::StrOp::StartsWith as u8
            || o == self::str::StrOp::EndsWith as u8
            || o == self::str::StrOp::Includes as u8
    )
}

/// Whether the intrinsic named by `wire_byte` indexes its receiver's bytes by
/// character position and returns a scalar — the shape the JIT can serve from
/// a byte pointer hoisted out of a loop.
pub fn intrinsic_is_char_index(wire_byte: u8) -> bool {
    let (domain, op) = wire::decode(wire_byte);
    domain == IntrinsicDomain::Str as u8
        && (op == self::str::StrOp::CharCodeAt as u8 || op == self::str::StrOp::CodePointAt as u8)
}

/// Wire byte for a core-class instance method that dispatches as an
/// intrinsic (receiver statically typed as that core class). `None` keeps
/// the method on the generic native op-id path.
pub fn core_method_intrinsic(class: &str, method: &str) -> Option<u8> {
    let entries: &[(&str, u8)] = match class {
        "str" => self::str::METHOD_ENTRIES,
        "int" => self::int::METHOD_ENTRIES,
        "Map" => self::collections::MAP_METHOD_ENTRIES,
        "Set" => self::collections::SET_METHOD_ENTRIES,
        _ => return None,
    };
    entries
        .iter()
        .find(|(name, _)| *name == method)
        .map(|&(_, wire)| wire)
}
