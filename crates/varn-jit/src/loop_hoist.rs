//! Loop-invariant array-guard hoisting (see `docs/PERF_STRUCTURAL_OPTS.md`,
//! Proyecto A). Identifies innermost loops that index a single
//! loop-invariant array through statically-array-typed opcodes
//! (`ArrayGetIndex`/`ArrayLength` — never plain `GetIndex`, which is only
//! emitted when the checker could NOT prove the receiver is an array) and
//! plans a cached fast path: the receiver-guard chain
//! (`array_fast::emit_resolve_array_payload`) is resolved once into a
//! reserved register instead of on every iteration.
//!
//! GC safety: eligibility requires the entire loop body be **provably
//! allocation-free** (see [`body_is_alloc_free`] — an allowlist, not a
//! denylist of "known-bad" ops, since missing one allocating opcode would
//! silently reopen a use-after-free). The preheader (`compiler.rs`) forces
//! any pending collection via `codegen::jumps::emit_gc_safepoint_check`
//! *before* caching, so nursery is fresh at that point; since nothing in an
//! eligible body can allocate, nothing during the loop's execution can move
//! or promote the cached array, so the loop's own back-edge safepoint never
//! needs to re-check. (An earlier design re-resolved on the back-edge's
//! "GC ran" branch instead of upfront — sound in principle, but a paired
//! benchmark showed a severe, GC-frequency-correlated slowdown from that
//! code path that wasn't fully root-caused; this simpler, statically-proven
//! design has no such path and measures at parity with baseline.)

use varn_core::OpCode;
use varn_types::bytecode::decode;
use varn_types::chunk::PoolEntry;
use varn_types::loop_analysis::{instr_offsets, natural_loops, NaturalLoop};

use crate::assembler::Reg;

/// Reserved physical register for the active loop's cached array-box
/// pointer. Not part of `regalloc::ALLOC_REGS` — see `registers.rs`: on
/// non-Windows every other callee-saved register is already claimed (4 for
/// general allocation + frame base + int tag), so `RegMap` must give up one
/// general-purpose slot to make this available, and only does so for
/// functions that actually have a hoist plan (see
/// `RegMap::from_bytecode`'s `reserve_cache` parameter).
pub(crate) const LOOP_ARRAY_CACHE_REG: Reg = Reg::R13;

/// One hoisted loop: `obj_vreg` is the sole invariant array touched in
/// `[header_offset, latch_offset]` (inclusive, `chunk.code` word offsets).
pub(crate) struct HoistPlan {
    pub obj_vreg: u8,
    pub header_offset: usize,
    pub latch_offset: usize,
}

impl HoistPlan {
    pub fn contains(&self, ip: usize) -> bool {
        ip >= self.header_offset && ip <= self.latch_offset
    }
}

/// `OpCode::Loop` is not exclusively a real loop back-edge in Varn's
/// bytecode: the SSA backend also uses it as a generic backward "goto" —
/// e.g. `break` immediately before a `return` gets compiled as a jump
/// straight to a shared `Return` instruction that happens to sit earlier in
/// the linearized code, which satisfies `collect_back_edges`' purely
/// numeric "target < this instruction" shape without being a loop at all.
/// That's harmless for the backend's live-range widening (over-widening is
/// merely conservative) but fatal for hoisting: the preheader is emitted
/// assuming the header is reached by *falling through* from the linearly
/// preceding code on first entry, and every jump that targets the header is
/// patched to *skip* the preheader. A "loop" whose header has no
/// fall-through predecessor (only reached via jumps, as with the shared
/// `Return` above) makes the preheader dead code — so the cache register is
/// never actually initialized, and the guarded fast path at each candidate
/// site races ahead reading garbage. Require an actual fall-through
/// predecessor to rule these out.
fn header_reachable_by_fallthrough(code: &[u16], offsets: &[usize], header_instr: usize) -> bool {
    let Some(prev_instr) = header_instr.checked_sub(1) else {
        return true; // function's first instruction — always entered directly
    };
    let prev_offset = offsets[prev_instr];
    !matches!(
        OpCode::from_u16(code[prev_offset]),
        Some(OpCode::Jump | OpCode::Loop | OpCode::Return | OpCode::Throw | OpCode::Yield)
    )
}

/// Every opcode a hoisted loop body may contain. Deliberately an allowlist:
/// pure arithmetic/comparison/control-flow plus in-bounds array read/write
/// (in-bounds `ArraySetIndex` never reallocates — only append/out-of-bounds
/// stores do, and those fall to the FFI helper, which is call-shaped and
/// already excluded). Anything not listed here — string ops, object/array
/// construction, property access that could hit a user getter, calls,
/// `ArrayPush`/`Pop`/`Extend` — is excluded, because ANY allocation inside
/// the loop is a GC opportunity the preheader's one-time safepoint doesn't
/// cover.
fn is_alloc_free_op(op: OpCode) -> bool {
    matches!(
        op,
        OpCode::LoadNull
            | OpCode::LoadTrue
            | OpCode::LoadFalse
            | OpCode::LoadInt
            | OpCode::LoadIntZero
            | OpCode::LoadIntOne
            | OpCode::LoadIntMinusOne
            | OpCode::LoadConst
            | OpCode::Move
            | OpCode::Add
            | OpCode::Sub
            | OpCode::Mul
            | OpCode::Div
            | OpCode::Mod
            | OpCode::Pow
            | OpCode::Negate
            | OpCode::Not
            | OpCode::AddImm
            | OpCode::SubImm
            | OpCode::AddInt
            | OpCode::SubInt
            | OpCode::MulInt
            | OpCode::DivInt
            | OpCode::ModInt
            | OpCode::PowInt
            | OpCode::AddFloat
            | OpCode::SubFloat
            | OpCode::MulFloat
            | OpCode::DivFloat
            | OpCode::ModFloat
            | OpCode::PowFloat
            | OpCode::BitAnd
            | OpCode::BitOr
            | OpCode::BitXor
            | OpCode::Shl
            | OpCode::Shr
            | OpCode::Ushr
            | OpCode::Eq
            | OpCode::Neq
            | OpCode::Lt
            | OpCode::Lte
            | OpCode::Gt
            | OpCode::Gte
            | OpCode::LtInt
            | OpCode::GtInt
            | OpCode::LteInt
            | OpCode::GteInt
            | OpCode::EqInt
            | OpCode::NeqInt
            | OpCode::LtFloat
            | OpCode::GtFloat
            | OpCode::LteFloat
            | OpCode::GteFloat
            | OpCode::EqFloat
            | OpCode::NeqFloat
            | OpCode::IsNull
            | OpCode::Jump
            | OpCode::JumpIfFalse
            | OpCode::JumpIfTrue
            | OpCode::Loop
            | OpCode::Return
            | OpCode::GetIndex
            | OpCode::ArrayGetIndex
            | OpCode::ArrayLength
            | OpCode::SetIndex
            | OpCode::ArraySetIndex
    )
}

fn body_is_alloc_free(code: &[u16], offsets: &[usize], lp: &NaturalLoop) -> bool {
    (lp.header..=lp.latch).all(|instr_idx| {
        OpCode::from_u16(code[offsets[instr_idx]]).is_some_and(is_alloc_free_op)
    })
}

/// Plan hoists for every eligible innermost loop in `code`. Conservative by
/// construction: skips loops with any instruction outside the
/// allocation-free allowlist (see [`is_alloc_free_op`]), more than one
/// candidate invariant array, a header with no fall-through predecessor
/// (see `header_reachable_by_fallthrough`), or that themselves contain a
/// nested loop (only the innermost loop of a nest gets the cache register —
/// see the design doc's "Registro de cache" section).
pub(crate) fn plan_hoists(code: &[u16], constants: &[PoolEntry]) -> Vec<HoistPlan> {
    let loops = natural_loops(code, constants);
    if loops.is_empty() {
        return Vec::new();
    }
    let offsets = instr_offsets(code, constants);

    // A pseudo-loop that fails the fall-through check (see
    // `header_reachable_by_fallthrough`) is not a real loop and must not
    // count as "nested" when deciding whether some OTHER loop is innermost
    // — otherwise a fake back-edge sitting inside a real loop's range would
    // wrongly disqualify the real loop from hoisting entirely.
    let is_real: Vec<bool> = loops
        .iter()
        .map(|lp| header_reachable_by_fallthrough(code, &offsets, lp.header))
        .collect();

    loops
        .iter()
        .enumerate()
        .filter_map(|(i, lp)| {
            if !is_real[i] || !body_is_alloc_free(code, &offsets, lp) {
                return None;
            }
            let is_innermost = !loops.iter().enumerate().any(|(j, other)| {
                j != i && is_real[j] && other.header >= lp.header && other.latch <= lp.latch
            });
            if !is_innermost {
                return None;
            }

            let mut candidate: Option<u8> = None;
            for instr_idx in lp.header..=lp.latch {
                let instr_offset = offsets[instr_idx];
                let op = OpCode::from_u16(code[instr_offset])?;
                if !matches!(op, OpCode::ArrayGetIndex | OpCode::ArrayLength) {
                    continue;
                }
                let info = decode(code, instr_offset, constants)?;
                let obj_reg = *info.uses.first()?;
                if !lp.is_invariant(obj_reg) {
                    continue;
                }
                match candidate {
                    None => candidate = Some(obj_reg),
                    Some(c) if c == obj_reg => {}
                    Some(_) => return None,
                }
            }

            candidate.map(|obj_vreg| HoistPlan {
                obj_vreg,
                header_offset: offsets[lp.header],
                latch_offset: offsets[lp.latch],
            })
        })
        .collect()
}
