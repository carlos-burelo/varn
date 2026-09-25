//! Everything compiled code can call back into, split by what the call is FOR
//! rather than by file size.
//!
//! These were two modules of 1513 and 1026 lines whose only shared property
//! was being written against `ExecCtx`. Splitting them by domain is safe in a
//! way it would not be elsewhere: the shared ABI list in `varn-jit` names
//! these functions, and `exec::ctx` re-exports them as a flat `ctx::jit_*`
//! surface, so a helper can move between these modules without any call site
//! or any generated code changing.

pub(crate) mod arith;
pub(crate) mod build;
pub(crate) mod calls;
pub(crate) mod classes;
pub(crate) mod construct;
pub(crate) mod exceptions;
pub(crate) mod frames;
pub(crate) mod ic;
pub(crate) mod indexing;
pub(crate) mod intrinsics;
pub(crate) mod modules;
pub(crate) mod natives;
pub(crate) mod objects;
pub(crate) mod props;
pub(crate) mod safepoint;
pub(crate) mod strings;
pub(crate) mod suspend;
pub(crate) mod types;
pub(crate) mod values;

// Flat re-export: `jit::helpers` and `exec::ctx` name these without caring
// which domain module they landed in.
pub(crate) use arith::*;
pub(crate) use build::*;
pub(crate) use calls::*;
pub(crate) use classes::*;
pub(crate) use construct::*;
pub(crate) use exceptions::*;
pub(crate) use frames::*;
pub(crate) use ic::*;
pub(crate) use indexing::*;
pub(crate) use intrinsics::*;
pub(crate) use modules::*;
pub(crate) use natives::*;
pub(crate) use objects::*;
pub(crate) use props::*;
pub(crate) use safepoint::*;
pub(crate) use strings::*;
pub(crate) use suspend::*;
pub(crate) use types::*;
pub(crate) use values::*;
