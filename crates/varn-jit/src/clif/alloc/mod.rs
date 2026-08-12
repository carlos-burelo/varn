//! Allocation-path lowering submodules for CLIF backend.

pub(crate) mod calls;
pub(crate) mod objects;
pub(crate) mod safepoints;

pub(crate) use calls::*;
pub(crate) use objects::*;
pub(crate) use safepoints::*;
