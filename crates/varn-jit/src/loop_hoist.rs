//! Loop-invariant array-guard hoisting (see `docs/notes/PERF_STRUCTURAL_OPTS.md`,
//! Proyecto A). Identifies innermost loops that index loop-invariant
//! arrays through statically-array-typed opcodes (`ArrayGetIndex`/
//! `ArrayLength` — never plain `GetIndex`, which is only emitted when the
//! checker could NOT prove the receiver is an array) and plans a cached
//! fast path: the receiver-guard chain (`array_fast::
//! emit_resolve_array_payload`) is resolved once into a reserved register
//! instead of on every iteration.
//!
//! Two kinds of invariance are recognized per candidate site (see
//! [`CacheSource`]):
//!   - `RegisterInvariant`: the object register is never redefined between
//!     the loop header and this use — its value at loop entry holds for
//!     every iteration.
//!   - `GlobalInvariant`: the object register IS redefined every iteration,
//!     but always by `LoadGlobalIdx` of the same global slot, and nothing
//!     in the loop stores to that slot — the *value* is loop-invariant even
//!     though the *register* is not (e.g. `a[row*n+k]` inside a loop that
//!     reloads global `a` every pass). The preheader must synthesize that
//!     load once (see `compiler.rs`) since the register may hold stale
//!     data at the point the preheader runs.
//! Classification is per SITE, not per register: the same physical register
//! can carry two different globals at different points in one iteration
//! (seen in matrix-multiply inner loops — `a` then `b` both land in the
//! same scratch vreg), so matching by register number alone would be
//! ambiguous. `HoistPlan::cache_reg_for` re-derives the site's source the
//! same way `plan_hoists` did and matches on the full (register, source)
//! pair.
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
//! design has no such path and measures at parity with baseline.) The same
//! allowlist rules out call-shaped instructions, which doubles as the
//! safety net for `GlobalInvariant`: a call could mutate the cached global
//! as a side effect, so a loop containing any call never reaches candidate
//! collection at all.

use varn_core::OpCode;
use varn_types::bytecode::decode;
use varn_types::chunk::PoolEntry;
use varn_types::loop_analysis::{instr_offsets, natural_loops, NaturalLoop};

/// Maximum simultaneously-cached arrays per loop.
#[cfg(target_os = "windows")]
const MAX_CANDIDATES: usize = 2;
#[cfg(not(target_os = "windows"))]
const MAX_CANDIDATES: usize = 1;

/// How a candidate site's array value is established to be the same across
/// every iteration. See module docs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CacheSource {
    RegisterInvariant,
    GlobalInvariant(u16),
}

pub struct HoistCandidate {
    pub obj_vreg: u8,
    pub source: CacheSource,
}

/// One hoisted loop: up to [`MAX_CANDIDATES`] invariant arrays touched in
/// `[header_offset, latch_offset]` (inclusive, `chunk.code` word offsets).
#[allow(dead_code)]
pub(crate) struct HoistPlan {
    pub candidates: Vec<HoistCandidate>,
    pub header_offset: usize,
    pub latch_offset: usize,
    latch_end_offset: usize,
}

impl HoistPlan {
    #[allow(dead_code)]
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
/// already excluded), plus `LoadGlobalIdx`/`StoreGlobalIdx`/
/// `DefineGlobalIdx` (needed for `GlobalInvariant` detection — these never
/// allocate, they read/write a fixed slot in the globals array). Anything
/// not listed here — string ops, object/array construction, property
/// access that could hit a user getter, calls, `ArrayPush`/`Pop`/`Extend`
/// — is excluded, because ANY allocation inside the loop is a GC
/// opportunity the preheader's one-time safepoint doesn't cover. Excluding
/// all call-shaped instructions also rules out a call mutating a globals
/// slot as a side effect mid-loop, which `GlobalInvariant` depends on.
pub fn is_alloc_free_op(op: OpCode) -> bool {
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
            | OpCode::LoadGlobalIdx
            | OpCode::StoreGlobalIdx
            | OpCode::DefineGlobalIdx
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
    (lp.header..=lp.latch)
        .all(|instr_idx| OpCode::from_u16(code[offsets[instr_idx]]).is_some_and(is_alloc_free_op))
}

/// Forward-scans `[start_offset, end_offset)` for a `Store/DefineGlobalIdx`
/// targeting `target_idx`. Used to invalidate a `GlobalInvariant` candidate
/// whose global IS written somewhere in the loop (by direct assignment —
/// call-shaped mutation is already excluded by `body_is_alloc_free`).
/// Conservative on decode failure: treats undecodable code as "written" so
/// a candidate is dropped rather than wrongly trusted.
fn global_stored_in_range(
    code: &[u16],
    constants: &[PoolEntry],
    start_offset: usize,
    end_offset: usize,
    target_idx: u16,
) -> bool {
    let mut off = start_offset;
    while off < end_offset {
        let Some(op) = OpCode::from_u16(code[off]) else {
            return true;
        };
        let Some(info) = decode(code, off, constants) else {
            return true;
        };
        if matches!(op, OpCode::StoreGlobalIdx | OpCode::DefineGlobalIdx) {
            // Encoding (see varn_types::bytecode::decode's StoreGlobalIdx/
            // DefineGlobalIdx arm and codegen/globals.rs's mirrored read):
            // word0 = opcode|src_reg, word1 unused-here, word2 = global idx.
            // `decode` already validated the instruction's full length fits
            // the buffer, so `off + 2` is in bounds here.
            if code[off + 2] == target_idx {
                return true;
            }
        }
        off += info.len;
    }
    false
}

/// Determine how `obj_reg`'s value at `site_offset` was most recently
/// established within `[header_offset, site_offset)`. Forward scan rather
/// than backward: bytecode is variable-length, so walking forward from a
/// known-good starting offset (the loop header) using each instruction's
/// own decoded length is simpler and just as precise as a reverse walk,
/// and produces the same "most recent def wins" result since later writes
/// overwrite `last_def` as the scan proceeds.
///
/// Returns `None` when the register's value at this site cannot be proven
/// loop-invariant: either it's redefined by something other than
/// `LoadGlobalIdx`, or it's fed by `LoadGlobalIdx` of a global that's
/// written somewhere else in the loop.
fn classify_site(
    code: &[u16],
    constants: &[PoolEntry],
    header_offset: usize,
    latch_end_offset: usize,
    site_offset: usize,
    obj_reg: u8,
) -> Option<CacheSource> {
    let mut last_def: Option<(OpCode, usize)> = None;
    let mut off = header_offset;
    while off < site_offset {
        let op = OpCode::from_u16(code[off])?;
        let info = decode(code, off, constants)?;
        if info.def == Some(obj_reg) {
            last_def = Some((op, off));
        }
        off += info.len;
    }
    match last_def {
        None => Some(CacheSource::RegisterInvariant),
        Some((OpCode::LoadGlobalIdx, def_off)) => {
            let idx = code[def_off + 1];
            if global_stored_in_range(code, constants, header_offset, latch_end_offset, idx) {
                None
            } else {
                Some(CacheSource::GlobalInvariant(idx))
            }
        }
        Some(_) => None,
    }
}

/// Full eligibility report for one natural loop, independent of whether it
/// ends up hoistable. `vn debug -p bytecode` renders this directly (see
/// `varn-debug::loop_diagnostics`) so an agent reading the dump sees
/// exactly what the JIT decided and why, instead of re-deriving it by hand
/// from the raw instruction stream.
pub struct LoopDiagnostic {
    pub header_offset: usize,
    pub latch_offset: usize,
    latch_end_offset: usize,
    /// Header has a genuine fall-through predecessor (see
    /// `header_reachable_by_fallthrough`). `false` means this loop can
    /// never be hoisted regardless of the other checks — Varn currently
    /// lowers NESTED for-loops to a test-at-bottom shape (entered via an
    /// explicit forward jump), which always fails this check; only
    /// top-level loops pass it today.
    pub is_real: bool,
    /// Body contains only opcodes in the allocation-free allowlist (see
    /// [`is_alloc_free_op`]) — required because the preheader's GC
    /// safepoint runs once, not per iteration.
    pub is_alloc_free: bool,
    /// No other real loop is nested inside this one — only the innermost
    /// loop of a nest gets the cache registers.
    pub is_innermost: bool,
    /// Populated only when `is_real && is_alloc_free && is_innermost`; one
    /// entry per array the loop will actually cache. Empty (with the flags
    /// above all true) means the loop qualified structurally but no
    /// `ArrayGetIndex`/`ArrayLength` site inside it resolved to a provably
    /// loop-invariant array.
    pub candidates: Vec<HoistCandidate>,
}

/// Diagnose every natural loop in `code`, hoistable or not. `plan_hoists`
/// is a thin filter over this — single source of truth, so the `vn debug`
/// view can never drift from what the JIT actually decides.
pub fn diagnose_loops(code: &[u16], constants: &[PoolEntry]) -> Vec<LoopDiagnostic> {
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
            let alloc_free = body_is_alloc_free(code, &offsets, lp);
            let is_innermost = !loops.iter().enumerate().any(|(j, other)| {
                j != i && is_real[j] && other.header >= lp.header && other.latch <= lp.latch
            });

            let header_offset = offsets[lp.header];
            let latch_offset = offsets[lp.latch];
            let latch_end_offset = latch_offset + decode(code, latch_offset, constants)?.len;

            let mut candidates: Vec<HoistCandidate> = Vec::with_capacity(MAX_CANDIDATES);
            if is_real[i] && alloc_free && is_innermost {
                for instr_idx in lp.header..=lp.latch {
                    let instr_offset = offsets[instr_idx];
                    let op = OpCode::from_u16(code[instr_offset])?;
                    if !matches!(op, OpCode::ArrayGetIndex | OpCode::ArrayLength) {
                        continue;
                    }
                    let info = decode(code, instr_offset, constants)?;
                    let obj_reg = *info.uses.first()?;

                    let source = if lp.is_invariant(obj_reg) {
                        CacheSource::RegisterInvariant
                    } else {
                        match classify_site(
                            code,
                            constants,
                            header_offset,
                            latch_end_offset,
                            instr_offset,
                            obj_reg,
                        ) {
                            Some(s) => s,
                            None => continue, // this site isn't cacheable; not fatal to the loop
                        }
                    };

                    if candidates
                        .iter()
                        .any(|c| c.obj_vreg == obj_reg && c.source == source)
                    {
                        continue; // already tracked
                    }

                    if candidates.len() >= MAX_CANDIDATES {
                        // More distinct invariant arrays than we have
                        // registers for. Keep what's already found rather
                        // than dropping everything: sites past the
                        // register budget just find no match in
                        // `cache_reg_for` and fall back to the
                        // always-correct guarded path, same as any
                        // non-hoisted site.
                        continue;
                    }
                    candidates.push(HoistCandidate {
                        obj_vreg: obj_reg,
                        source,
                    });
                }
            }

            Some(LoopDiagnostic {
                header_offset,
                latch_offset,
                latch_end_offset,
                is_real: is_real[i],
                is_alloc_free: alloc_free,
                is_innermost,
                candidates,
            })
        })
        .collect()
}

/// Plan hoists for every eligible innermost loop in `code`. Conservative by
/// construction: skips loops with any instruction outside the
/// allocation-free allowlist (see [`is_alloc_free_op`]), more than
/// [`MAX_CANDIDATES`] distinct invariant arrays, a header with no
/// fall-through predecessor (see `header_reachable_by_fallthrough`), or
/// that themselves contain a nested loop (only the innermost loop of a
/// nest gets the cache registers — see the design doc's "Registro de
/// cache" section).
#[allow(dead_code)]
pub(crate) fn plan_hoists(code: &[u16], constants: &[PoolEntry]) -> Vec<HoistPlan> {
    diagnose_loops(code, constants)
        .into_iter()
        .filter(|d| !d.candidates.is_empty())
        .map(|d| HoistPlan {
            candidates: d.candidates,
            header_offset: d.header_offset,
            latch_offset: d.latch_offset,
            latch_end_offset: d.latch_end_offset,
        })
        .collect()
}
