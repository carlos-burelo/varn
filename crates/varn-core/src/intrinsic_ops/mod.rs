pub mod collections;
pub mod int;
pub mod map;
pub mod math;
pub mod str;
pub mod wire;

pub use map::lookup as intrinsic_lookup;
pub use wire::{decode as intrinsic_decode, encode as intrinsic_encode, IntrinsicDomain};

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
