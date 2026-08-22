//! Allocation-path lowering submodules for CLIF backend.

pub(crate) mod arrays;
pub(crate) mod calls;
pub(crate) mod closures;
pub(crate) mod instances;
pub(crate) mod modules;
pub(crate) mod native;
pub(crate) mod safepoints;
pub(crate) mod tasks;

pub(crate) use arrays::*;
pub(crate) use calls::*;
pub(crate) use closures::*;
pub(crate) use instances::*;
pub(crate) use modules::*;
pub(crate) use native::*;
pub(crate) use safepoints::*;
pub(crate) use tasks::*;
