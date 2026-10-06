pub mod ast;
pub mod binds;
pub mod bytecode;
pub mod cap_trace;
pub mod clif;
pub mod colors;
pub mod consts;
pub mod error;
pub mod expr;
pub mod flags;
pub mod fmt;
pub mod loop_diagnostics;
pub mod modules;
pub mod phase;
pub mod phases;
pub mod registry;
pub mod render;
pub mod report;
pub mod scope;
pub mod selection;
pub mod summary;
pub mod symbols;
pub mod tiers;
pub mod tir;
pub mod tokens;
pub mod typeloss;
pub mod walk;

pub use cap_trace::debug_cap_trace;
pub use flags::{print_phases, DebugFlags};

pub fn resolved_copy(proto: &varn_types::FunctionProto) -> varn_types::FunctionProto {
    proto.clone()
}
