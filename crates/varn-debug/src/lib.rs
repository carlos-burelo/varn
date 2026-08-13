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
pub mod hir;
pub mod loop_diagnostics;
pub mod modules;
pub mod roots;
pub mod scope;
pub mod ssa;
pub mod summary;
pub mod symbols;
pub mod tiers;
pub mod tokens;

pub use cap_trace::debug_cap_trace;
pub use flags::{print_phases, DebugFlags};

/// A copy of `proto` in the shape the JIT actually sees at runtime.
///
/// The JIT-facing views (`-p tiers`, `-p bails`, `-p roots`, `-p clif`) compile
/// straight out of `emit`, but production never does: the VM rewrites every
/// name-keyed global access to its indexed form before a proto can run. Without
/// this, those views report `unsupported opcode LoadGlobal` bails for functions
/// that route perfectly in production — a diagnostic that invents failures is
/// worse than none.
///
/// The store is a throwaway: only the OPCODE shape reaches the lowering, and
/// static inspection links nothing (`NoLinker`), so the slot numbers it hands
/// out never have to match any live VM's.
pub fn resolved_copy(proto: &varn_types::FunctionProto) -> varn_types::FunctionProto {
    let mut copy = proto.clone();
    varn_vm::globals::resolve_in_proto(&mut copy, &mut varn_vm::GlobalStore::default());
    copy
}
