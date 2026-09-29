//! Host float capability probe for the SSA lowering.
//!
//! The SSA lowering (`from_ssa::numeric`) inlines `floor`/`ceil` as single
//! ISA instructions; the native float opcode family this module once lowered
//! died with the bytecode lowering.

use cranelift_codegen::isa::OwnedTargetIsa;

/// Whether this host can lower the `round` family (`floor`/`ceil`), which on
/// x86-64 is `roundsd` and needs SSE4.1.
///
/// Cranelift does NOT degrade gracefully here: lowering `floor` without the
/// feature panics inside the ISLE tables ("no rule matched for term
/// x64_round") rather than falling back to a libcall, so the check has to
/// happen before the instruction is built. A target without the flag (or a
/// non-x86 one, which has no `has_sse41` at all) keeps the helper path.
pub(super) fn has_round_support(isa: &OwnedTargetIsa) -> bool {
    isa.isa_flags()
        .iter()
        .find(|f| f.name == "has_sse41")
        .and_then(|f| f.as_bool())
        .unwrap_or(false)
}
