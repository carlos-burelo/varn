//! Allocation-path lowering submodules for CLIF backend.
//!
//! Only the shared pieces survive the bytecode-lowering removal: the
//! whole-function allocation scan (the size gate's leaf-safe rule reads it)
//! and the back-edge GC poll both lowerings share.

pub(crate) mod safepoints;

pub(crate) use safepoints::*;
