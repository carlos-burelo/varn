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
pub mod registry;
pub mod render;
pub mod report;
pub mod roots;
pub mod scope;
pub mod selection;
pub mod summary;
pub mod symbols;
pub mod tiers;
pub mod tir;
pub mod tokens;
pub mod typeloss;

pub use cap_trace::debug_cap_trace;
pub use flags::{print_phases, DebugFlags};

/// A copy of `proto` in the shape the JIT sees at runtime.
///
/// The compiler now emits the indexed global opcodes directly
/// (`LoadGlobalIdx` / `StoreGlobalIdx` / `LoadNativeGlobalIdx`), so the
/// JIT-facing views (`-p tiers`, `-p bails`, `-p roots`, `-p clif`) already see
/// the production shape and this is a plain clone. Kept as the single call
/// point in case that changes again.
pub fn resolved_copy(proto: &varn_types::FunctionProto) -> varn_types::FunctionProto {
    proto.clone()
}
