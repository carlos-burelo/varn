
# Typed Unboxed Ints in JIT Loops (Proyecto B, scoped v1) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Eliminate the per-instruction NaN-box tag/untag dance (`docs/PERF_STRUCTURAL_OPTS.md`, "Proyecto B") for ONE loop-carried `int` accumulator register per JIT-compiled function, cutting `alu_int`-style arithmetic loops from 5 x86 instructions per op toward 2, closing part of the 2.5x `alu_int` gap vs Node.

**Architecture:** Reuses Proyecto A's proven-safe pattern exactly (`varn-jit/src/loop_hoist.rs`, `varn_types::loop_analysis`): find eligible innermost natural loops whose body is provably allocation-free (new stricter allowlist — arithmetic/comparison/control-flow only, **no** array ops, so this feature never shares a loop with Proyecto A's array-cache hoist), force the existing GC safepoint check once in the preheader, then keep exactly one loop-carried `SlotKind::Int` register permanently untagged (sign-extended raw i64) in a dedicated callee-saved register (`R13`) for the loop's entire duration. Because the body is alloc-free, the per-iteration back-edge safepoint provably never takes its flush branch during the loop's execution — so the untagged register is **never** written to memory while untagged, closing the GC-misinterprets-raw-bits hazard without any new GC-awareness code.

**Deviation from the original design-doc sketch (`docs/PERF_STRUCTURAL_OPTS.md` Proyecto B):** that sketch proposed a general `RegState::{Boxed,UntaggedInt}` machine with promote/rebox at arbitrary boundaries (calls, stores, comparisons, spills) and loop-carry across the back-edge — the doc itself flags this as "el más alto riesgo del roadmap." Three concrete hazards found while reading the current codegen (`arith.rs`, `compare.rs`, `regalloc.rs`, `codegen/jumps.rs::emit_gc_safepoint_check`) make that general version unsafe to build directly:
1. `emit_gc_safepoint_check` calls `emit_flush_all`, which writes every mapped register's **current bit pattern** to its VM-stack memory slot unconditionally before the GC runs. A register in `UntaggedInt` state at that point would hand the GC a raw i64 that may accidentally match a heap-pointer NaN-box tag pattern — silent corruption, not a crash.
2. `AddInt`/`SubInt` today use a cheaper encoding (tag-subtract + mask, 4 instructions, operates directly on boxed bits) that is **not bit-compatible** with the sign-extended encoding `MulInt`/`ModInt`/`AddImm`/`SubImp` already use (`shl 16; sar 16`). There is no single existing "untagged form" to promote into today.
3. Comparisons (`LtInt` etc.) use a third representation (`shl 16` only, no `sar` — value scaled into the high bits, order-preserving for `cmp`) that also differs from the arithmetic sign-extended form.

v1 sidesteps all three: (1) is solved structurally (safepoint flush path is provably unreachable during an alloc-free loop, exactly like Proyecto A already relies on); (2)/(3) are solved by **scoping to one dedicated cache register with one canonical representation** (sign-extended, i.e. the `MulInt`/`ModInt` form) and writing an explicit representation-adapter for the one place that matters (a comparison reading the cache register applies its own `shl 16` on a copy — see Task 5). Multi-register / cross-call / cross-boundary promotion is explicitly out of scope for v1 and noted as future work.

**Tech Stack:** Rust, x86-64 JIT assembler (`varn-jit/src/assembler.rs`, no new primitives needed — this plan only uses `shl_reg_imm8`/`sar_reg_imm8`/`shr_reg_imm8`/`add_reg_reg`/`sub_reg_reg`/`imul_reg_reg`/`or_reg_reg`/`mov_reg_reg`/`cmp_reg_reg`, all of which already exist and are used by the code this plan modifies).

## Global Constraints

- `tests/main.vn` must stay 670/670 (`vn run tests/main.vn`) after every task — this repo's standing regression gate (`<validation>` in `CLAUDE.md`).
- `VARN_NO_JIT=1 vn run tests/main.vn` must produce bit-identical output to the JIT path at every task (tier-identity, see `[[varn-int-semantics-i48]]`).
- Build with `cargo build -p varn-cli --bin vn` (workspace default features are sufficient for `varn-jit`/`varn-cli`; only `varn-builtins` needs `--features runtime`, per `[[varn-builtins-runtime-feature]]`, unrelated to this plan).
- Every register pushed/popped in the JIT prologue/epilogue MUST go through `RegMap.used_phys` (never a bare separate `asm.push`) — the ~30 call sites across `codegen/` that compute 16-byte stack alignment from `regmap.used_phys.len()` before FFI/GC calls will silently desync otherwise (this bit Proyecto A once, see `PERF_STRUCTURAL_OPTS.md`'s postmortem).
- No new permanent feature flag / dual code path: once this plan lands, the untagged fast path is just how eligible loops compile — there is no toggle.
- Wrap semantics are i48, sign-extended (`[[varn-int-semantics-i48]]`) — the cache register must be re-masked to valid i48 range after **every** update, not just at box-out time.

---

### Task 1: Extract shared "eligible innermost loop with fall-through header" helper

Both Proyecto A (`loop_hoist.rs`) and this plan need identical loop-selection logic (innermost natural loop, header reached by fall-through, not a fake backward-goto). Today it only exists inlined in `loop_hoist.rs::plan_hoists`. Extract it once so Proyecto B doesn't duplicate it (DRY, per `CLAUDE.md` core rules).

**Files:**
- Modify: `crates/varn-types/src/loop_analysis.rs`
- Modify: `crates/varn-jit/src/loop_hoist.rs:70-79,168-232` (use the extracted helper, no behavior change)
- Test: `crates/varn-types/src/loop_analysis.rs` (new `#[cfg(test)]` module — first unit tests in this file)

**Interfaces:**
- Produces: `varn_types::loop_analysis::header_reachable_by_fallthrough(code: &[u16], offsets: &[usize], header_instr: usize) -> bool` (moved verbatim from `loop_hoist.rs`, same signature) and `varn_types::loop_analysis::innermost_real_loops(code: &[u16], constants: &[PoolEntry]) -> Vec<usize>` returning the **indices into `natural_loops(code, constants)`** that are both real (fall-through header) and innermost — callers still call `natural_loops` themselves and index into it with the returned indices, so no new struct/ownership issues.

- [ ] **Step 1: Move `header_reachable_by_fallthrough` into `loop_analysis.rs`**

Cut the function verbatim from `crates/varn-jit/src/loop_hoist.rs:70-79` into `crates/varn-types/src/loop_analysis.rs`, making it `pub` instead of private:

```rust
/// `OpCode::Loop` is not exclusively a real loop back-edge in Varn's
/// bytecode: the SSA backend also uses it as a generic backward "goto".
/// A "loop" whose header has no fall-through predecessor is not a real
/// loop and must be excluded from any header-relative codegen (preheader
/// emission assumes entry-by-fallthrough).
pub fn header_reachable_by_fallthrough(code: &[u16], offsets: &[usize], header_instr: usize) -> bool {
    let Some(prev_instr) = header_instr.checked_sub(1) else {
        return true;
    };
    let prev_offset = offsets[prev_instr];
    !matches!(
        OpCode::from_u16(code[prev_offset]),
        Some(OpCode::Jump | OpCode::Loop | OpCode::Return | OpCode::Throw | OpCode::Yield)
    )
}
```

- [ ] **Step 2: Add `innermost_real_loops` to `loop_analysis.rs`**

```rust
/// Indices into `natural_loops(code, constants)`'s result that are both
/// "real" (fall-through header, see `header_reachable_by_fallthrough`) and
/// innermost (contain no other real loop). Shared by every JIT loop-scoped
/// optimization that needs exactly one entry point per eligible loop.
pub fn innermost_real_loops(code: &[u16], constants: &[PoolEntry]) -> Vec<usize> {
    let loops = natural_loops(code, constants);
    if loops.is_empty() {
        return Vec::new();
    }
    let offsets = instr_offsets(code, constants);
    let is_real: Vec<bool> = loops
        .iter()
        .map(|lp| header_reachable_by_fallthrough(code, &offsets, lp.header))
        .collect();

    (0..loops.len())
        .filter(|&i| {
            is_real[i]
                && !loops.iter().enumerate().any(|(j, other)| {
                    j != i
                        && is_real[j]
                        && other.header >= loops[i].header
                        && other.latch <= loops[i].latch
                })
        })
        .collect()
}
```

- [ ] **Step 3: Write unit tests for both functions**

Add at the bottom of `crates/varn-types/src/loop_analysis.rs`. These need real bytecode arrays — the simplest way to get one is to hand-assemble a tiny loop using existing `OpCode`/`decode` shapes. Use a trivial `while` pattern: `LoadIntZero r0; Loop <back to same instr>` is enough to exercise the header/latch/innermost logic without needing a full compiler:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use varn_core::OpCode;

    fn back_edge(target_instr_word_offset: usize, from_word_offset: usize) -> [u16; 3] {
        let back = (from_word_offset + 3 - target_instr_word_offset) as u32;
        [OpCode::Loop as u16, (back >> 16) as u16, (back & 0xFFFF) as u16]
    }

    #[test]
    fn single_loop_is_real_and_innermost() {
        // word 0: LoadIntZero r0 (1 word)
        // word 1..4: Loop back to word 0 (3 words)
        let mut code = vec![OpCode::LoadIntZero as u16];
        code.extend_from_slice(&back_edge(0, 1));
        let constants: Vec<PoolEntry> = Vec::new();

        let loops = natural_loops(&code, &constants);
        assert_eq!(loops.len(), 1);
        let idx = innermost_real_loops(&code, &constants);
        assert_eq!(idx, vec![0]);
    }

    #[test]
    fn fake_backward_goto_after_return_is_not_real() {
        // word 0: Return (1 word) <- fake "header", only reached via jump
        // word 1..4: Loop back to word 0 (3 words) — mimics break-before-return collapse
        let mut code = vec![OpCode::Return as u16];
        code.extend_from_slice(&back_edge(0, 1));
        let constants: Vec<PoolEntry> = Vec::new();

        let idx = innermost_real_loops(&code, &constants);
        assert!(idx.is_empty(), "loop whose header follows a Return must not be real");
    }
}
```

- [ ] **Step 4: Run the new unit tests**

Run: `cargo test -p varn-types loop_analysis`
Expected: both tests PASS.

- [ ] **Step 5: Rewire `loop_hoist.rs` to use the shared helpers, no behavior change**

In `crates/varn-jit/src/loop_hoist.rs`, delete the local `header_reachable_by_fallthrough` (now imported from `varn_types::loop_analysis`) and replace the manual `is_real`/innermost computation in `plan_hoists` (lines ~186-204) with a call to `innermost_real_loops`:

```rust
use varn_types::loop_analysis::{innermost_real_loops, instr_offsets, natural_loops, NaturalLoop};
// ... header_reachable_by_fallthrough import no longer needed locally

pub(crate) fn plan_hoists(code: &[u16], constants: &[PoolEntry]) -> Vec<HoistPlan> {
    let loops = natural_loops(code, constants);
    if loops.is_empty() {
        return Vec::new();
    }
    let offsets = instr_offsets(code, constants);
    let eligible_indices = innermost_real_loops(code, constants);

    eligible_indices
        .into_iter()
        .filter_map(|i| {
            let lp = &loops[i];
            if !body_is_alloc_free(code, &offsets, lp) {
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
```

Remove the now-dead `is_real` computation and the local `header_reachable_by_fallthrough` function entirely (it lives in `loop_analysis.rs` now).

- [ ] **Step 6: Build and run the full regression suite**

Run: `cargo build -p varn-cli --bin vn`
Run: `vn run tests/main.vn`
Expected: `670/670`, identical to before this task (pure refactor, no behavior change).
Run: `VARN_NO_JIT=1 vn run tests/main.vn`
Expected: same pass count as the pre-existing baseline (the one pre-existing unrelated failure in `42-stdlib-comprehensive-test.vn` noted in `[[varn-dispatch-abi-redesign]]` is expected and unrelated).

- [ ] **Step 7: Commit**

```bash
git add crates/varn-types/src/loop_analysis.rs crates/varn-jit/src/loop_hoist.rs
git commit -m "refactor(jit): extract shared innermost-real-loop selection into loop_analysis"
```

---

### Task 2: Untag eligibility analysis (`untag_hoist.rs`)

Pure analysis, no codegen yet. Finds at most one loop-carried `SlotKind::Int` register per function worth caching untagged.

**Files:**
- Create: `crates/varn-jit/src/untag_hoist.rs`
- Modify: `crates/varn-jit/src/lib.rs` (register the new module)
- Test: inline `#[cfg(test)]` module in `untag_hoist.rs`

**Interfaces:**
- Consumes: `varn_types::loop_analysis::{natural_loops, instr_offsets, innermost_real_loops, NaturalLoop}` (Task 1), `varn_types::bytecode::decode`, `varn_types::register_meta::{RegisterMeta, SlotKind}`, `crate::loop_hoist::HoistPlan` (for mutual-exclusion check).
- Produces: `pub(crate) const UNTAG_INT_CACHE_REG: Reg = Reg::R13;`, `pub(crate) struct UntagPlan { pub vreg: u8, pub header_offset: usize, pub latch_offset: usize, pub exit_offsets: Vec<usize> }` with `pub fn contains(&self, ip: usize) -> bool`, and `pub(crate) fn plan_untag(code: &[u16], constants: &[PoolEntry], register_meta: &[RegisterMeta], array_hoist_plans: &[HoistPlan]) -> Option<UntagPlan>` — later tasks (3, 4) consume this exact signature and the `UNTAG_INT_CACHE_REG` constant.

- [ ] **Step 1: Write the allowlist and eligibility scan**

```rust
//! Loop-scoped untagged-int caching (see `docs/PERF_STRUCTURAL_OPTS.md`,
//! Proyecto B, scoped v1). Caches ONE loop-carried `int`-typed register in
//! `UNTAG_INT_CACHE_REG`, sign-extended and un-boxed, for the entire
//! duration of a single eligible innermost loop, eliminating the per-op
//! NaN-box tag/untag dance for that register's arithmetic chain.
//!
//! Deliberately disjoint from `loop_hoist` (Proyecto A): the allowlist below
//! contains NO array opcodes, so a loop can never be claimed by both this
//! module and `loop_hoist` at once — no interaction between the array-cache
//! register and this cache register is possible by construction.
//!
//! GC safety follows the identical argument `loop_hoist` already
//! establishes: the body is provably allocation-free, so once the preheader
//! forces one safepoint check up front, the per-iteration back-edge
//! safepoint can never actually take its flush branch for the rest of the
//! loop's execution — meaning the cache register, which is never part of
//! `RegMap.map` and is never flushed to memory anywhere in an eligible
//! loop's body, is never visible to the GC while holding untagged bits.

use varn_core::OpCode;
use varn_types::bytecode::decode;
use varn_types::chunk::PoolEntry;
use varn_types::loop_analysis::{innermost_real_loops, instr_offsets, natural_loops};
use varn_types::register_meta::{RegisterMeta, SlotKind};

use crate::assembler::Reg;
use crate::loop_hoist::HoistPlan;

/// Reserved physical register for the active loop's cached untagged int.
/// Distinct from `loop_hoist::LOOP_ARRAY_CACHE_REG` (`R14`) so a function
/// containing one loop eligible for each optimization can use both at once
/// (see `regalloc.rs::RegMap::from_bytecode`, Task 3).
pub(crate) const UNTAG_INT_CACHE_REG: Reg = Reg::R13;

pub(crate) struct UntagPlan {
    pub vreg: u8,
    pub header_offset: usize,
    pub latch_offset: usize,
    /// Word offsets of every instruction inside `[header_offset,
    /// latch_offset]` that leaves the loop (a conditional jump whose target
    /// is outside the loop, or a `Return`) — the cache register MUST be
    /// re-boxed and written back to its real slot immediately before each
    /// of these, since nothing after this point is guaranteed to be inside
    /// the alloc-free-proven region anymore.
    pub exit_offsets: Vec<usize>,
}

impl UntagPlan {
    pub fn contains(&self, ip: usize) -> bool {
        ip >= self.header_offset && ip <= self.latch_offset
    }
}

/// Every opcode this optimization's loop bodies may contain. A strict
/// subset of `loop_hoist::is_alloc_free_op` with all array/indexing ops
/// removed — see the module doc comment for why that disjointness matters.
fn is_untag_eligible_op(op: OpCode) -> bool {
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
            | OpCode::IsNull
            | OpCode::Jump
            | OpCode::JumpIfFalse
            | OpCode::JumpIfTrue
            | OpCode::Loop
            | OpCode::Return
    )
}

fn body_is_untag_eligible(code: &[u16], offsets: &[usize], header: usize, latch: usize) -> bool {
    (header..=latch).all(|instr_idx| {
        OpCode::from_u16(code[offsets[instr_idx]]).is_some_and(is_untag_eligible_op)
    })
}
```

- [ ] **Step 2: Write the exit-point and candidate-register scan**

```rust
/// Word offsets, within `[header, latch]`, of every jump/return that leaves
/// the loop: a `JumpIfFalse`/`JumpIfTrue` whose target is outside
/// `[header_offset, latch_offset]`, or any `Return`. `Loop`'s own back-edge
/// (the one targeting `header`) is never an exit and is excluded.
fn find_exit_offsets(
    code: &[u16],
    constants: &[PoolEntry],
    offsets: &[usize],
    header: usize,
    latch: usize,
) -> Vec<usize> {
    let header_offset = offsets[header];
    let latch_offset = offsets[latch];
    let mut exits = Vec::new();
    for instr_idx in header..=latch {
        let instr_offset = offsets[instr_idx];
        let Some(op) = OpCode::from_u16(code[instr_offset]) else {
            continue;
        };
        match op {
            OpCode::Return => exits.push(instr_offset),
            // `JumpIfFalse`/`JumpIfTrue` are the loop's own condition check
            // (exits when the target is outside the loop). `Jump` is
            // ALSO an exit candidate: `break` lowers to a bare
            // unconditional `Terminator::Jump { target: break_target }`
            // (`varn-opt/src/ssa/build/stmt.rs`'s `HirStmt::Break` arm),
            // not a conditional jump — so a `break` inside an
            // untag-eligible loop is exactly as much an exit as the
            // loop's own condition check, and must rebox the cache
            // register before it fires. `continue` also lowers to a bare
            // `Jump`, but its target is the loop's own test/update point,
            // which is INSIDE `[header, latch]` — the `target <
            // header_offset || target > latch_offset` check below already
            // excludes it correctly without needing to special-case it.
            OpCode::JumpIfFalse | OpCode::JumpIfTrue | OpCode::Jump => {
                let Some(info) = decode(code, instr_offset, constants) else {
                    continue;
                };
                if let Some(target) = info.jump_target {
                    if target < header_offset || target > latch_offset {
                        exits.push(instr_offset);
                    }
                }
            }
            _ => {}
        }
    }
    exits
}

/// Find the single best untag-eligible loop-carried `int` register across
/// every eligible innermost loop in `code`, or `None` if there is nothing
/// to cache. "Best" = appears as an operand of the most arithmetic/
/// comparison instructions within its loop (ties broken by first-seen).
/// v1 caps at exactly one register/one loop per function — see the plan's
/// design-doc note on why (register budget, representation simplicity).
pub(crate) fn plan_untag(
    code: &[u16],
    constants: &[PoolEntry],
    register_meta: &[RegisterMeta],
    array_hoist_plans: &[HoistPlan],
) -> Option<UntagPlan> {
    let loops = natural_loops(code, constants);
    if loops.is_empty() {
        return None;
    }
    let offsets = instr_offsets(code, constants);
    let eligible_indices = innermost_real_loops(code, constants);

    let mut best: Option<(UntagPlan, u32)> = None;

    for i in eligible_indices {
        let lp = &loops[i];
        let header_offset = offsets[lp.header];
        // Disjoint from Proyecto A by construction — skip any loop A already claimed.
        if array_hoist_plans
            .iter()
            .any(|p| p.header_offset == header_offset)
        {
            continue;
        }
        if !body_is_untag_eligible(code, &offsets, lp.header, lp.latch) {
            continue;
        }

        let mut freq: std::collections::HashMap<u8, u32> = std::collections::HashMap::new();
        for instr_idx in lp.header..=lp.latch {
            let instr_offset = offsets[instr_idx];
            let Some(info) = decode(code, instr_offset, constants) else {
                continue;
            };
            let Some(op) = OpCode::from_u16(code[instr_offset]) else {
                continue;
            };
            if !matches!(
                op,
                OpCode::Add | OpCode::Sub | OpCode::Mul | OpCode::Div | OpCode::Mod | OpCode::Pow
                    | OpCode::AddInt | OpCode::SubInt | OpCode::MulInt | OpCode::DivInt
                    | OpCode::ModInt | OpCode::PowInt | OpCode::AddImm | OpCode::SubImm
                    | OpCode::LtInt | OpCode::GtInt | OpCode::LteInt | OpCode::GteInt
                    | OpCode::EqInt | OpCode::NeqInt
            ) {
                continue;
            }
            if let Some(d) = info.def {
                if register_meta.get(d as usize).map_or(false, |m| m.kind == SlotKind::Int)
                    && !lp.is_invariant(d)
                {
                    *freq.entry(d).or_insert(0) += 1;
                }
            }
            for &u in &info.uses {
                if register_meta.get(u as usize).map_or(false, |m| m.kind == SlotKind::Int)
                    && !lp.is_invariant(u)
                {
                    *freq.entry(u).or_insert(0) += 1;
                }
            }
        }

        let Some((&vreg, &count)) = freq.iter().max_by_key(|(_, &c)| c) else {
            continue;
        };

        if best.as_ref().is_none_or(|(_, best_count)| count > *best_count) {
            let exit_offsets = find_exit_offsets(code, constants, &offsets, lp.header, lp.latch);
            best = Some((
                UntagPlan {
                    vreg,
                    header_offset,
                    latch_offset: offsets[lp.latch],
                    exit_offsets,
                },
                count,
            ));
        }
    }

    best.map(|(plan, _)| plan)
}
```

Note: `info.jump_target` — verify the exact field name on `varn_types::bytecode::decode`'s return type before writing this; if it's not already surfaced (the decoder may only expose `def`/`uses`/`call_args`/`len`), add it: `JumpIfFalse`/`JumpIfTrue`/`Jump`/`Loop` all encode a 32-bit relative word offset identically to the inline decoding already done in `codegen/jumps.rs::emit_jumps` — mirror that arithmetic (`ip + 2 +/- offset`) inside `decode` and expose it as `pub jump_target: Option<usize>` on the decode result. This is the one piece of this task that touches the single-source-of-truth decoder (`[[varn-bytecode-decoder-single-source]]`) — confirm with a grep for the struct definition before assuming it's missing.

- [ ] **Step 3: Register the module**

In `crates/varn-jit/src/lib.rs`, add `mod untag_hoist;` next to the existing `mod loop_hoist;` (or wherever crate-level `mod` declarations live).

- [ ] **Step 4: Write unit tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use varn_types::register_meta::RegisterMeta;

    fn int_meta(n: usize) -> Vec<RegisterMeta> {
        vec![RegisterMeta { kind: SlotKind::Int }; n]
    }

    #[test]
    fn simple_accumulator_loop_is_eligible() {
        // r0 = 0
        // header: r0 = r0 + r1 (AddInt r0, r0, r1)   [3 words: op, first_reg<<8, (src1<<8|src2)]
        // Loop back to header
        let mut code = vec![OpCode::LoadIntZero as u16];
        // AddInt: first_reg=0 in high byte of word0, then next word packs src1<<8|src2
        code.push((OpCode::AddInt as u16) | (0u16 << 8));
        code.push((0u16 << 8) | 1u16);
        let header_instr = 1; // AddInt is instr index 1 (after LoadIntZero)
        let from_word = code.len() + 2; // Loop's own 3 words come next
        let back = (from_word + 3 - 1) as u32; // back to word offset of AddInt (word 1)
        code.push(OpCode::Loop as u16);
        code.push((back >> 16) as u16);
        code.push((back & 0xFFFF) as u16);

        let constants: Vec<PoolEntry> = Vec::new();
        let meta = int_meta(2);
        let plan = plan_untag(&code, &constants, &meta, &[]);
        assert!(plan.is_some(), "accumulator loop should be untag-eligible");
        assert_eq!(plan.unwrap().vreg, 0);
    }

    #[test]
    fn loop_with_array_op_is_not_eligible() {
        // Body containing ArrayGetIndex must be rejected outright.
        let mut code = vec![(OpCode::ArrayGetIndex as u16) | (0u16 << 8)];
        code.push((1u16 << 8) | 0u16);
        let back = (code.len() + 3) as u32;
        code.push(OpCode::Loop as u16);
        code.push((back >> 16) as u16);
        code.push((back & 0xFFFF) as u16);

        let constants: Vec<PoolEntry> = Vec::new();
        let meta = int_meta(2);
        assert!(plan_untag(&code, &constants, &meta, &[]).is_none());
    }
}
```

(These hand-assembled byte sequences must be checked against the exact word-packing `varn_types::bytecode::decode` expects — read `decode`'s source alongside writing these tests and adjust the packing helpers if the layout differs from the `(op)(first_reg<<8)`/`(src1<<8|src2)` shape inferred from `codegen/arith.rs`.)

- [ ] **Step 5: Run tests**

Run: `cargo test -p varn-jit untag_hoist`
Expected: both tests PASS. Iterate on the hand-assembled bytecode until they do — this is the single place in the plan where getting the raw word packing wrong is likely; do not proceed to Task 3 until these are solid, since every later task builds on `plan_untag` being correct.

- [ ] **Step 6: Build check (no wiring yet, so no behavior change)**

Run: `cargo build -p varn-cli --bin vn`
Run: `vn run tests/main.vn`
Expected: `670/670`, unchanged — `plan_untag` exists but nothing calls it from `compiler.rs` yet.

- [ ] **Step 7: Commit**

```bash
git add crates/varn-jit/src/untag_hoist.rs crates/varn-jit/src/lib.rs
git commit -m "feat(jit): add loop-carried int untag eligibility analysis (unused yet)"
```

---

### Task 3: `RegMap` reservation for the untag cache register

**Files:**
- Modify: `crates/varn-jit/src/regalloc.rs:6,19-115`

**Interfaces:**
- Consumes: `crate::untag_hoist::UNTAG_INT_CACHE_REG` (Task 2).
- Produces: `RegMap::from_bytecode(code, constants, reserve_cache: bool, reserve_untag: bool)` (signature change — one call site to update in `compiler.rs`, Task 4) and `pub untag_reg: Option<Reg>` field on `RegMap`, populated iff `reserve_untag`.

- [ ] **Step 1: Update the withholding logic to compose both reservations**

Replace the current block in `RegMap::from_bytecode` (`crates/varn-jit/src/regalloc.rs:82-87`):

```rust
let alloc_regs = if reserve_cache {
    debug_assert_eq!(ALLOC_REGS[3], crate::loop_hoist::LOOP_ARRAY_CACHE_REG);
    &ALLOC_REGS[..3]
} else {
    ALLOC_REGS
};
```

with (order-independent withholding — see the plan header's rationale for why a plain slice-prefix doesn't compose):

```rust
debug_assert_eq!(ALLOC_REGS[2], crate::untag_hoist::UNTAG_INT_CACHE_REG);
debug_assert_eq!(ALLOC_REGS[3], crate::loop_hoist::LOOP_ARRAY_CACHE_REG);
let mut alloc_regs: Vec<Reg> = vec![ALLOC_REGS[0], ALLOC_REGS[1]];
if !reserve_untag {
    alloc_regs.push(ALLOC_REGS[2]);
}
if !reserve_cache {
    alloc_regs.push(ALLOC_REGS[3]);
}
```

- [ ] **Step 2: Update the function signature and struct**

```rust
pub struct RegMap {
    map: HashMap<usize, Reg>,
    pub used_phys: Vec<Reg>,
    pub cache_reg: Option<Reg>,
    /// Set only when this function has a loop-carried untagged-int plan
    /// (see `untag_hoist`). Mirrors `cache_reg` exactly: withheld from
    /// general allocation only when actually used, rides in `used_phys`
    /// (never `map`) so it inherits the existing 16-byte-alignment
    /// accounting at every FFI/GC call site for free.
    pub untag_reg: Option<Reg>,
}

impl RegMap {
    pub fn from_bytecode(
        code: &[u16],
        constants: &[varn_types::chunk::PoolEntry],
        reserve_cache: bool,
        reserve_untag: bool,
    ) -> Self {
        // ... (unchanged frequency ranking) ...

        // ... (Step 1's alloc_regs block) ...

        let n = alloc_regs.len().min(ranked.len());
        let mut map = HashMap::new();
        let mut used_phys = Vec::new();

        for (i, (vreg, _freq)) in ranked.iter().take(n).enumerate() {
            if ranked[i].1 >= 2 {
                map.insert(*vreg, alloc_regs[i]);
                used_phys.push(alloc_regs[i]);
            }
        }

        let cache_reg = reserve_cache.then_some(crate::loop_hoist::LOOP_ARRAY_CACHE_REG);
        if let Some(reg) = cache_reg {
            used_phys.push(reg);
        }
        let untag_reg = reserve_untag.then_some(crate::untag_hoist::UNTAG_INT_CACHE_REG);
        if let Some(reg) = untag_reg {
            used_phys.push(reg);
        }
        Self { map, used_phys, cache_reg, untag_reg }
    }
    // ... rest unchanged ...
}
```

- [ ] **Step 3: Build (call site still passes 3 args — will fail to compile until Task 4)**

Run: `cargo build -p varn-jit`
Expected: FAIL — `compiler.rs`'s call to `RegMap::from_bytecode` now has the wrong arity. This is expected; Task 4 fixes the call site. Confirm the error is exactly the arity mismatch (no other break).

- [ ] **Step 4: Commit (as part of Task 4's build-passing commit, since this task alone doesn't compile)**

Do not commit yet — proceed directly to Task 4, then commit both together, since Task 3 alone leaves the workspace in a non-building state and `CLAUDE.md`'s "never leave code half-finished" applies at the commit boundary, not the task boundary.

---

### Task 4: Wire `untag_hoist` into `compiler.rs` — preheader promotion + exit reboxing

**Files:**
- Modify: `crates/varn-jit/src/compiler.rs`
- Modify: `crates/varn-jit/src/codegen/mod.rs:43-63` (`CodegenCtx` gains a field)

**Interfaces:**
- Consumes: `untag_hoist::plan_untag` (Task 2), `RegMap::from_bytecode` new signature (Task 3), `codegen::jumps::emit_gc_safepoint_check` (existing, reused verbatim).
- Produces: `CodegenCtx.untag_plan: Option<&'a UntagPlan>` — Task 5's `arith.rs`/`compare.rs` changes read this field.

- [ ] **Step 1: Add the field to `CodegenCtx`**

In `crates/varn-jit/src/codegen/mod.rs`, add to the `CodegenCtx` struct and imports:

```rust
use crate::untag_hoist::UntagPlan;

pub(crate) struct CodegenCtx<'a> {
    // ... existing fields ...
    pub hoist_plans: &'a [HoistPlan],
    /// At most one loop-carried untagged-int cache plan for this function
    /// (see `untag_hoist`). `None` for the overwhelming majority of
    /// functions.
    pub untag_plan: Option<&'a UntagPlan>,
    pub safe_int_call_opt: bool,
}
```

- [ ] **Step 2: Compute the plan and fix `RegMap::from_bytecode`'s call site in `compile_proto`**

In `crates/varn-jit/src/compiler.rs`:

```rust
use crate::untag_hoist::plan_untag;

// ... after `let hoist_plans = plan_hoists(code, &proto.chunk.constants);` ...
let untag_plan = plan_untag(code, &proto.chunk.constants, &proto.register_meta, &hoist_plans);

let mut asm = Assembler::new();
let regmap = RegMap::from_bytecode(
    code,
    &proto.chunk.constants,
    !hoist_plans.is_empty(),
    untag_plan.is_some(),
);
```

Add `untag_plan: untag_plan.as_ref(),` to the `CodegenCtx` literal built a few lines below.

- [ ] **Step 3: Emit the preheader (forced safepoint + promote to `UNTAG_INT_CACHE_REG`)**

In the main dispatch loop's existing header-check block (right after the Proyecto A hoist-plan check, so both can fire for the same `ip` if a function has two independent eligible loops — they never share a loop by construction, but their headers could both land on the same `cctx.ip` iteration only if it's literally the same instruction, which the mutual-exclusion check in `plan_untag` already prevents):

```rust
if let Some(plan) = cctx.untag_plan.filter(|p| p.header_offset == cctx.ip) {
    crate::codegen::jumps::emit_gc_safepoint_check(&mut cctx.asm, &cctx.regmap, cctx.helpers);
    // Promote: load the current boxed value, strip the tag, sign-extend
    // into UNTAG_INT_CACHE_REG. Canonical form = MulInt/ModInt's existing
    // representation (`shl 16; sar 16` on the boxed bits).
    crate::regalloc::emit_load(&mut cctx.asm, crate::untag_hoist::UNTAG_INT_CACHE_REG, plan.vreg as usize, &cctx.regmap);
    cctx.asm.shl_reg_imm8(crate::untag_hoist::UNTAG_INT_CACHE_REG, 16);
    cctx.asm.sar_reg_imm8(crate::untag_hoist::UNTAG_INT_CACHE_REG, 16);
}
```

- [ ] **Step 4: Emit rebox-and-writeback at every exit point, before the instruction's normal codegen runs**

Immediately before the existing `let op = OpCode::from_u8(raw_op as u8).unwrap();` dispatch in the main loop, add:

```rust
if let Some(plan) = cctx.untag_plan {
    if plan.exit_offsets.contains(&cctx.ip) {
        // Re-box UNTAG_INT_CACHE_REG and write it back to vreg's real
        // slot (register or memory) before this exit-taking instruction
        // runs, so the exit's own codegen (a jump-condition load, or
        // Return's read) sees a valid boxed value like any other slot.
        use crate::registers::REG_INT_TAG;
        cctx.asm.shl_reg_imm8(crate::untag_hoist::UNTAG_INT_CACHE_REG, 16);
        cctx.asm.shr_reg_imm8(crate::untag_hoist::UNTAG_INT_CACHE_REG, 16);
        cctx.asm.or_reg_reg(crate::untag_hoist::UNTAG_INT_CACHE_REG, REG_INT_TAG);
        crate::regalloc::emit_store(&mut cctx.asm, crate::untag_hoist::UNTAG_INT_CACHE_REG, plan.vreg as usize, &cctx.regmap);
        // Immediately re-promote so the cache stays valid if this
        // exit-check turns out NOT to be taken at runtime (both
        // JumpIfFalse/JumpIfTrue branches continue past this point in the
        // instruction stream; only the taken branch actually leaves).
        cctx.asm.shl_reg_imm8(crate::untag_hoist::UNTAG_INT_CACHE_REG, 16);
        cctx.asm.sar_reg_imm8(crate::untag_hoist::UNTAG_INT_CACHE_REG, 16);
    }
}
```

Note the box→store→re-promote sequence: an exit_offset instruction is a **conditional** jump (or `Return`, which never falls through, so the trailing re-promote after `Return` is dead code but harmless — the assembler position after a `Return`'s codegen is only reached via other control flow, never fallthrough from `Return` itself). For `JumpIfFalse`/`JumpIfTrue`, the box must be visible on the taken (exiting) branch, but the loop continues on the not-taken branch using the SAME cached register — hence re-promoting immediately after unconditionally, before either branch is emitted, is correct and cheap (2 extra instructions per exit check, only paid once per iteration for loops with a conditional exit check, which is every `while`/`for` loop's condition test).

- [ ] **Step 5: Build**

Run: `cargo build -p varn-cli --bin vn`
Expected: SUCCESS (Task 3's arity break is now fixed).

- [ ] **Step 6: Regression check — still behavior-neutral**

Run: `vn run tests/main.vn`
Expected: `670/670`. At this point the cache register is computed and reboxed but Task 5 hasn't wired any arithmetic op to actually READ/WRITE it as a fast path yet, so this is pure overhead with no observable effect — correctness must be unchanged.
Run: `VARN_NO_JIT=1 vn run tests/main.vn` — same as Task 1's baseline.

- [ ] **Step 7: Commit**

```bash
git add crates/varn-jit/src/compiler.rs crates/varn-jit/src/codegen/mod.rs crates/varn-jit/src/regalloc.rs
git commit -m "feat(jit): wire untag-hoist preheader promotion and exit reboxing (no fast path yet)"
```

---

### Task 5: Untagged fast path for `AddInt`/`SubInt`/`MulInt`/`ModInt` and `Int` comparisons

This is the task that actually produces the instruction-count win.

**Files:**
- Modify: `crates/varn-jit/src/codegen/arith.rs:411-455` (`emit_int_arith`), `:283-383` (`emit_mod_int`)
- Modify: `crates/varn-jit/src/codegen/compare.rs:212-259` (`emit_int_compare`)

**Interfaces:**
- Consumes: `ctx.untag_plan` (Task 4), `crate::untag_hoist::UNTAG_INT_CACHE_REG`.

- [ ] **Step 1: Add a small shared helper for "is this register the active cache register in the current plan"**

At the top of `arith.rs` (or a tiny shared module both files can import — given both `arith.rs` and `compare.rs` already have their own `slot_is_int`/`slot_is_float` duplicated, follow the existing pattern rather than introducing a new shared module for one predicate):

```rust
fn is_cached(ctx: &CodegenCtx, ip_in_plan_range: bool, reg: usize) -> bool {
    ip_in_plan_range
        && ctx.untag_plan.map_or(false, |p| p.vreg as usize == reg)
}
```

- [ ] **Step 2: Rewrite `emit_int_arith` to use the cache register when eligible**

Replace `crates/varn-jit/src/codegen/arith.rs:411-455`:

```rust
fn emit_int_arith(ctx: &mut CodegenCtx, op: OpCode, first_reg: usize) {
    let w1 = ctx.code[ctx.ip];
    ctx.ip += 1;
    let src1 = (w1 >> 8) as usize;
    let src2 = (w1 & 0xFF) as usize;

    let in_plan = ctx
        .untag_plan
        .map_or(false, |p| p.contains(ctx.ip - 2) /* this instruction's own ip */);
    let dst_is_cache = in_plan && ctx.untag_plan.map_or(false, |p| p.vreg as usize == first_reg);
    let src1_is_cache = in_plan && ctx.untag_plan.map_or(false, |p| p.vreg as usize == src1);
    let src2_is_cache = in_plan && ctx.untag_plan.map_or(false, |p| p.vreg as usize == src2);

    // v1 only fuses the single-cache-register case: exactly one operand is
    // the cache register and the destination IS that same register (the
    // `acc = acc OP x` accumulator shape — the pattern this whole project
    // targets). Anything else (e.g. `acc` read but written to a DIFFERENT
    // register, or both operands happening to be non-cache) falls through
    // to the existing boxed path unchanged.
    if dst_is_cache && (src1_is_cache || src2_is_cache) && !(src1_is_cache && src2_is_cache) {
        let asm = &mut ctx.asm;
        let regmap = &ctx.regmap;
        let cache = crate::untag_hoist::UNTAG_INT_CACHE_REG;
        let other_src = if src1_is_cache { src2 } else { src1 };

        // Promote the OTHER operand into a scratch register (cache is
        // already sign-extended untagged).
        emit_load(asm, Reg::R11, other_src, regmap);
        asm.shl_reg_imm8(Reg::R11, 16);
        asm.sar_reg_imm8(Reg::R11, 16);

        match op {
            OpCode::AddInt => asm.add_reg_reg(cache, Reg::R11),
            OpCode::SubInt => {
                if src1_is_cache {
                    asm.sub_reg_reg(cache, Reg::R11);
                } else {
                    // cache is src2: result = other - cache
                    asm.sub_reg_reg(Reg::R11, cache);
                    asm.mov_reg_reg(cache, Reg::R11);
                }
            }
            OpCode::MulInt => asm.imul_reg_reg(cache, Reg::R11),
            _ => unreachable!(),
        }
        // Re-mask to valid i48 sign-extended range — required for i48 wrap
        // semantics even in the untagged fast path (see plan's Global
        // Constraints and `[[varn-int-semantics-i48]]`).
        asm.shl_reg_imm8(cache, 16);
        asm.sar_reg_imm8(cache, 16);
        return;
    }

    // Fallback: existing boxed-form codegen, unchanged.
    let asm = &mut ctx.asm;
    let regmap = &ctx.regmap;
    emit_load(asm, Reg::Rax, src1, regmap);
    emit_load(asm, Reg::R11, src2, regmap);
    match op {
        OpCode::AddInt => {
            asm.add_reg_reg(Reg::Rax, Reg::R11);
            asm.sub_reg_reg(Reg::Rax, crate::registers::REG_INT_TAG);
            asm.shl_reg_imm8(Reg::Rax, 16);
            asm.shr_reg_imm8(Reg::Rax, 16);
            asm.or_reg_reg(Reg::Rax, crate::registers::REG_INT_TAG);
        }
        OpCode::SubInt => {
            asm.sub_reg_reg(Reg::Rax, Reg::R11);
            asm.shl_reg_imm8(Reg::Rax, 16);
            asm.shr_reg_imm8(Reg::Rax, 16);
            asm.or_reg_reg(Reg::Rax, crate::registers::REG_INT_TAG);
        }
        OpCode::MulInt => {
            asm.shl_reg_imm8(Reg::Rax, 16);
            asm.sar_reg_imm8(Reg::Rax, 16);
            asm.shl_reg_imm8(Reg::R11, 16);
            asm.sar_reg_imm8(Reg::R11, 16);
            asm.imul_reg_reg(Reg::Rax, Reg::R11);
            asm.shl_reg_imm8(Reg::Rax, 16);
            asm.shr_reg_imm8(Reg::Rax, 16);
            asm.or_reg_reg(Reg::Rax, crate::registers::REG_INT_TAG);
        }
        _ => unreachable!(),
    }
    emit_store(asm, Reg::Rax, first_reg, regmap);
}
```

**Correctness-critical detail:** `ctx.ip - 2` above assumes `emit_int_arith` is entered with `ctx.ip` already advanced past the instruction's 2 words (matching the existing code's `let w1 = ctx.code[ctx.ip]; ctx.ip += 1;` pattern — confirm the exact pre-advance state by re-reading `emit_arith`'s dispatcher in the same file before relying on this offset; get it wrong and `plan.contains()` silently always returns false, which is safe — no fast path taken — or always true for the wrong instructions, which is NOT safe. Add a debug assertion cross-checking against `first_reg`/`plan.vreg` consistency if there's any doubt.

- [ ] **Step 3: Apply the same pattern to `emit_mod_int`**

In `emit_mod_int` (`arith.rs:283-383`), the existing code already does `shl_reg_imm8`/`sar_reg_imm8` sign-extension of both operands before the `idiv` (lines 322-328) — when the dividend (`src1`, loaded into `Rax`) is the cache register, skip its promotion (it's already sign-extended); it is NOT valid to skip the divisor's zero-check or its own promotion, since a divisor of 0 must still fall back to the FFI helper regardless of which operand is cached. Concretely: guard the `asm.shl_reg_imm8(Reg::Rax, 16); asm.sar_reg_imm8(Reg::Rax, 16);` pair (only) behind `if !src1_is_cache`, and load `Rax` from the cache register directly (`asm.mov_reg_reg(Reg::Rax, cache)`) instead of `emit_load` when `src1_is_cache`. The result, after `idiv`, must still be re-masked and re-tagged for the STORE (ModInt's result is not itself kept in the cache register in v1 — only `AddInt`/`SubInt`/`MulInt` chains stay in the cache per Step 2's `dst_is_cache` gate; `ModInt`'s result always writes back through the existing boxed `emit_store` path). This keeps `ModInt` simpler (partial win: skips ONE operand's promotion when it's the cached accumulator, e.g. `acc = acc % M`) without extending the "stays untagged across ops" chain through `ModInt` — a `ModInt` result immediately re-entering the cache would require re-verifying the representation is still valid input to `idiv` next iteration, deferred to future work.

- [ ] **Step 4: Apply the representation-adapter to `emit_int_compare`**

In `emit_int_compare` (`compare.rs:212-259`), when `src1` or `src2` is the cache register, replace `emit_load(asm, Reg::Rax, src1, regmap)` (or `R11`/`src2`) with `asm.mov_reg_reg(Reg::Rax, cache)` (skipping the memory/regmap load, since the value already lives in a register) — but do **not** skip the subsequent `asm.shl_reg_imm8(Reg::Rax, 16)`: comparisons use a different representation (scaled into the high bits, no `sar`) than the arithmetic cache form (fully sign-extended). Applying `shl 16` to an already-sign-extended value still produces the correct scaled-comparison form (the top 16 bits get overwritten by the shift regardless of what was there before), so this is safe with no special-casing beyond skipping the `emit_load`.

- [ ] **Step 5: Build**

Run: `cargo build -p varn-cli --bin vn`
Expected: SUCCESS.

- [ ] **Step 6: Regression check**

Run: `vn run tests/main.vn` → expect `670/670`.
Run: `VARN_NO_JIT=1 vn run tests/main.vn` → same baseline as Task 1.

This is the task most likely to surface a correctness bug (wrong representation assumption, off-by-one on `ctx.ip`, wrong operand-order for non-commutative `Sub`). Do not proceed to Task 6 until both regression commands are clean.

- [ ] **Step 7: Commit**

```bash
git add crates/varn-jit/src/codegen/arith.rs crates/varn-jit/src/codegen/compare.rs
git commit -m "perf(jit): untagged fast path for accumulator-shaped int arithmetic in eligible loops"
```

---

### Task 6: Correctness tests — wrap, early exit, GC pressure

**Files:**
- Modify: `tests/01-arithmetic.vn`
- Modify: `tests/22-recursion.vn` or a new dedicated section — pick whichever existing file already exercises loops with early `return` (check before adding a new import to `tests/main.vn`; prefer extending an existing file per repo convention over adding a 54th import).

**Interfaces:** none (integration tests only).

- [ ] **Step 1: Add an i48-wrap loop test to `tests/01-arithmetic.vn`**

Append:

```
{
  let acc = 140737488355327 // 2^47 - 1, max positive i48
  let i = 0
  while i < 4 {
    acc = acc + 1
    i = i + 1
  }
  assert("int wrap in loop matches i48 semantics", acc === -140737488355325)
}
```

(Compute the expected wrapped value from `[[varn-int-semantics-i48]]`'s documented wrap rule — verify by running under `VARN_NO_JIT=1` first if unsure of the exact wrapped constant, then assert the JIT path matches it, rather than guessing the arithmetic by hand.)

- [ ] **Step 2: Add an early-return-from-loop test**

```
fn sumUntil(limit: int): int {
  let acc = 0
  let i = 0
  while i < 1000 {
    acc = acc + i
    if acc > limit {
      return acc
    }
    i = i + 1
  }
  return acc
}
assert("early return from untag-eligible loop", sumUntil(10) >= 10)
```

This exercises Task 4's `exit_offsets` rebox path via `Return` inside the loop body.

- [ ] **Step 3: Add a GC-pressure test mirroring Proyecto A's validation style**

```
fn triggerAllocations(): void {
  let junk = []
  let n = 0
  while n < 200 {
    junk.push({ x: n })
    n = n + 1
  }
}

fn accumulate(times: int): int {
  let acc = 0
  let i = 0
  while i < times {
    acc = acc + i * 3 - (i % 7)
    i = i + 1
  }
  return acc
}

{
  let i = 0
  let allMatch = true
  while i < 500 {
    triggerAllocations() // build nursery pressure right before entering the untag-eligible loop
    let result = accumulate(1000)
    if result !== 1493250 { allMatch = false }
    i = i + 1
  }
  assert("untag-eligible loop correct under repeated GC pressure", allMatch)
}
```

(Compute `1493250` — or whatever the real closed form is — by running this snippet under `VARN_NO_JIT=1` first and using that as the expected value, exactly like Proyecto A's own validation did with its 5000-forced-GC test.)

- [ ] **Step 4: Run the suite under both JIT and interpreter**

Run: `vn run tests/main.vn` → expect `670/670` **plus the new asserts** (count will be `670 + N` where `N` is however many `assert(...)` calls were added — confirm the new total, don't assume it stays `670`).
Run: `VARN_NO_JIT=1 vn run tests/main.vn` → same new total, bit-identical results.

- [ ] **Step 5: Commit**

```bash
git add tests/01-arithmetic.vn tests/22-recursion.vn
git commit -m "test(jit): cover i48 wrap, early-return, and GC-pressure correctness for untag-hoisted loops"
```

---

### Task 7: Paired benchmark validation + docs

**Files:**
- Modify: `docs/PERF_STRUCTURAL_OPTS.md` (add an "Implementación" subsection under Proyecto B, mirroring Proyecto A's postmortem style)

**Interfaces:** none.

- [ ] **Step 1: Recreate the paired bench harness**

Per `[[varn-honest-benchmark-baseline]]`, the paired `bench.vn`/`bench.js` suite must be recreated per session in scratchpad (it is not checked into the repo). Recreate the `alu_int` workload specifically (100M-iteration loop with `% 1e9+7`, matching the existing baseline's shape) in `C:\Users\x\AppData\Local\Temp\claude\...\scratchpad\bench.vn` / `bench.js`.

- [ ] **Step 2: Run the paired benchmark, back-to-back with Node, same-moment**

Run: `vn bench --runs 9 <scratchpad>/bench.vn` and the equivalent `node <scratchpad>/bench.js`, interleaved (per the measurement-discipline rule in `[[varn-honest-benchmark-baseline]]`: this machine throttles ~2.2x under sustained load, only same-moment ratios are comparable).

- [ ] **Step 3: Compare against the 2.5x baseline**

Record the new `alu_int` ratio. Given v1's scope (only the accumulator's `Add`/`Sub`/`Mul` chain within one loop gets fused; `Mod` only skips one operand's promotion; the loop-condition comparison is unchanged cost), do not expect the full 2.5x→1.5x the original design doc projected — that projection assumed the general multi-register, cross-boundary design this plan explicitly scoped down from. Report the actual measured number honestly, per `<performance_rules>` in `CLAUDE.md` ("no afirmar mejoras de rendimiento sin medición").

- [ ] **Step 4: Write the "Implementación" section**

Add to `docs/PERF_STRUCTURAL_OPTS.md` under "## Proyecto B", following the exact structure Proyecto A's own postmortem uses (deviations found during implementation, files touched, measured bench numbers, validation performed). Include the three hazards found during this plan's design phase (GC-flush-of-raw-bits, Add/Sub vs Mul/Mod representation mismatch, comparison's third representation) as the documented reason v1 is scoped to a single cache register instead of the general design.

- [ ] **Step 5: Commit**

```bash
git add docs/PERF_STRUCTURAL_OPTS.md
git commit -m "docs(perf): record Proyecto B v1 scope, measured alu_int impact, and implementation deviations"
```

---

## Explicit non-goals (future work, not this plan)

- Multiple cached untagged registers per function (v1 caps at one; would need a second dedicated register and per-loop selection among competing candidates).
- Untagging loop-**invariant** int operands (only loop-carried/written registers are cached in v1; an invariant operand read every iteration still pays full promotion each time).
- Extending `is_untag_eligible_op` to cover `GetFixedField`/array ops so `prop_mono`/`array_sum` could also benefit — deliberately excluded from v1 to keep this plan disjoint from Proyecto A and avoid the array-guard/untagged-index interaction that was never analyzed.
- `fib35` will **not** improve from this plan — it is recursive (`Call`-shaped), and `RegMap` already stops all register allocation at the first call-shaped instruction; there is no loop for this optimization to attach to.
