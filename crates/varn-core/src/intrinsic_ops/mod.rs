//! Builtins the compiler emits as `OpCode::Intrinsic`: today the `std:math`
//! functions, identified on the wire by their [`math::MathOp`] byte. None of
//! them touches the heap, so a loop calling them stays allocation-free.

pub mod map;
pub mod math;

pub use map::lookup as intrinsic_lookup;
