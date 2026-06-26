pub mod map;
pub mod math;
pub mod wire;

pub use map::lookup as intrinsic_lookup;
pub use wire::{decode as intrinsic_decode, encode as intrinsic_encode, IntrinsicDomain};
