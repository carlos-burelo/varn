//! Loop-invariant code motion over typed SSA.
//!
//! Scope is deliberately conservative — a hoist must be a pure win with no
//! semantic risk:
//!
//! * Hoisted instructions are non-throwing and allocation-free: `Binary`
//!   add/sub/mul and comparisons on proven `Int`/`Float` operand classes,
//!   and `Unary` neg/not on the same. A test-at-top loop can run zero
//!   iterations, so anything that could throw (div/mod/pow, dynamic ops)
//!   or allocate must stay put.
//! * Literals are deliberately NOT hoisted. Rematerializing one is a single
//!   cheap dispatch, while holding it in a register across the loop competes
//!   with everything else live there. Measured on `tests/main.vn`, 20 runs,
//!   interleaved A/B: hoisting literals cost ~12% (127.9 ms vs 114.6 ms p50
//!   e2e) and bought nothing measurable on an object-field loop that load
//!   hoisting alone already sped up 1.4x.
//! * `LoadGlobalIdx` hoists only out of "transparent" loops — no calls, no
//!   global stores, no opaque effects anywhere in the loop — since any call
//!   can rebind a global.
//! * `GetFixedField` and `ArrayGetIndex` hoist only out of loops where every
//!   instruction is pure ([`super::dce::is_pure`]) — no store of any kind and
//!   no user code, so a repeat read is guaranteed to see the same value.
//!   Neither can throw from the preheader: the checker only emits
//!   `GetFixedField` for a receiver it proved is a non-nullable class (a
//!   nullable one lowers to `GetPropertyMaybe`), and an out-of-range
//!   `ArrayGetIndex` yields null rather than raising.
//! * The destination is the loop's unique entry predecessor, and only when
//!   that predecessor ends in an unconditional jump to the header: it runs
//!   exactly when the loop is entered, and it dominates every loop block.
//!
//! Loops are natural loops of back-edges (`P → H` where `H` dominates `P`).
//! The pass-manager fixpoint reruns this pass, so invariants cascade
//! outward through nested loops without inner/outer ordering logic here.

use crate::hir::{HirBinOp, HirType, HirUnOp};
use crate::ssa::ir::{BlockId, InstKind, SsaFunc, Terminator};

pub fn run(func: &mut SsaFunc) -> bool {
    let n = func.blocks.len();
    if n < 2 {
        return false;
    }

    let dom = dominators(func);
    let mut changed = false;

    // Back-edges grouped by header: a loop with several `continue` latches
    // is ONE natural loop (the union over its back-edges) — flooding from a
    // single latch would under-approximate membership and mis-hoist.
    let mut loops: rustc_hash::FxHashMap<BlockId, Vec<BlockId>> = rustc_hash::FxHashMap::default();
    for b in 0..n {
        for succ in successors(&func.blocks[b].term) {
            if dominates(&dom, succ.0 as usize, b) {
                loops.entry(succ).or_default().push(BlockId(b as u32));
            }
        }
    }

    let mut headers: Vec<BlockId> = loops.keys().copied().collect();
    headers.sort_by_key(|h| h.0);
    for header in headers {
        let latches = &loops[&header];
        changed |= hoist_loop(func, latches, header);
    }

    changed
}

fn hoist_loop(func: &mut SsaFunc, latches: &[BlockId], header: BlockId) -> bool {
    let n = func.blocks.len();

    // Natural loop: header + everything that reaches a latch without
    // passing through the header.
    let mut in_loop = vec![false; n];
    in_loop[header.0 as usize] = true;
    let mut stack = latches.to_vec();
    while let Some(b) = stack.pop() {
        if in_loop[b.0 as usize] {
            continue;
        }
        in_loop[b.0 as usize] = true;
        for &p in &func.blocks[b.0 as usize].preds {
            stack.push(p);
        }
    }

    // Unique entry predecessor that unconditionally jumps to the header.
    let entries: Vec<BlockId> = func.blocks[header.0 as usize]
        .preds
        .iter()
        .copied()
        .filter(|p| !in_loop[p.0 as usize])
        .collect();
    let [pre] = entries.as_slice() else {
        return false;
    };
    let pre = *pre;
    if !matches!(
        func.blocks[pre.0 as usize].term,
        Terminator::Jump { target, .. } if target == header
    ) {
        return false;
    }

    // Values defined inside the loop (params and inst dests).
    let mut def_in_loop = vec![false; func.values.len()];
    let mut facts = LoopFacts {
        globals_stable: true,
        memory_stable: true,
    };
    for (b, block) in func.blocks.iter().enumerate() {
        if !in_loop[b] {
            continue;
        }
        for &p in &block.params {
            def_in_loop[p.0 as usize] = true;
        }
        for inst in &block.insts {
            if let Some(d) = inst.dest {
                def_in_loop[d.0 as usize] = true;
            }
            if !is_transparent(&inst.kind) {
                facts.globals_stable = false;
            }
            if !super::dce::is_pure(&inst.kind) {
                facts.memory_stable = false;
            }
        }
    }

    // Iterate to a fixpoint so chains of invariants hoist together.
    let mut changed = false;
    loop {
        let mut moved_any = false;
        for b in 0..n {
            if !in_loop[b] || b == pre.0 as usize {
                continue;
            }
            let mut i = 0;
            while i < func.blocks[b].insts.len() {
                let inst = &func.blocks[b].insts[i];
                let movable = hoistable(&inst.kind, facts)
                    && operands(&inst.kind)
                        .iter()
                        .all(|v| !def_in_loop[v.0 as usize]);
                if movable {
                    let inst = func.blocks[b].insts.remove(i);
                    if let Some(d) = inst.dest {
                        def_in_loop[d.0 as usize] = false;
                    }
                    func.blocks[pre.0 as usize].insts.push(inst);
                    moved_any = true;
                    changed = true;
                } else {
                    i += 1;
                }
            }
        }
        if !moved_any {
            break;
        }
    }
    changed
}

/// What the loop body as a whole guarantees, which decides how much of it a
/// single instruction is allowed to leave.
#[derive(Debug, Clone, Copy)]
struct LoopFacts {
    /// Nothing in the loop can rebind a global.
    globals_stable: bool,
    /// Nothing in the loop writes memory or runs user code, so a repeated
    /// load reads the same value on every iteration.
    memory_stable: bool,
}

/// Non-throwing, allocation-free, effect-free — safe to run even when the
/// loop body executes zero times.
fn hoistable(kind: &InstKind, facts: LoopFacts) -> bool {
    match kind {
        InstKind::GetFixedField { .. } | InstKind::ArrayGetIndex { .. } => facts.memory_stable,

        InstKind::Binary { op, ty, .. } => {
            let typed = matches!(ty, HirType::Int | HirType::Float);
            let safe_op = matches!(
                op,
                HirBinOp::Add
                    | HirBinOp::Sub
                    | HirBinOp::Mul
                    | HirBinOp::Eq
                    | HirBinOp::Ne
                    | HirBinOp::Lt
                    | HirBinOp::Le
                    | HirBinOp::Gt
                    | HirBinOp::Ge
            );
            typed && safe_op
        }
        InstKind::Unary { op, ty, .. } => {
            matches!(ty, HirType::Int | HirType::Float | HirType::Bool)
                && matches!(op, HirUnOp::Neg | HirUnOp::Not)
        }
        InstKind::LoadGlobal(_) => facts.globals_stable,
        InstKind::IsNull { .. } | InstKind::IsArray { .. } | InstKind::GetEnumTag { .. } => true,
        InstKind::MakeClosure { upvalues, .. } => upvalues.is_empty(),
        _ => false,
    }
}

/// Whether an instruction is incapable of rebinding a global, so
/// `LoadGlobal` stays invariant across it. Whitelist, not blacklist:
/// anything that can run user code (calls, property access — getters and
/// setters — generic indexing) or store a global is opaque. Fixed fields
/// and typed array slots are plain memory, never accessors.
fn is_transparent(kind: &InstKind) -> bool {
    matches!(
        kind,
        InstKind::ConstInt(_)
            | InstKind::ConstFloat(_)
            | InstKind::ConstBool(_)
            | InstKind::ConstStr(_)
            | InstKind::ConstChar(_)
            | InstKind::ConstDecimal(_)
            | InstKind::ConstBigInt(_)
            | InstKind::ConstNull
            | InstKind::Binary { .. }
            | InstKind::Unary { .. }
            | InstKind::LoadGlobal(_)
            | InstKind::LoadUpvalue(_)
            | InstKind::StoreUpvalue { .. }
            | InstKind::GetFixedField { .. }
            | InstKind::SetFixedField { .. }
            | InstKind::ArrayGetIndex { .. }
            | InstKind::ArraySetIndex { .. }
            | InstKind::IsNull { .. }
            | InstKind::IsArray { .. }
            | InstKind::GetEnumTag { .. }
            | InstKind::MakeClosure { .. }
    )
}

fn operands(kind: &InstKind) -> Vec<crate::ssa::ir::Value> {
    crate::ssa::verify::inst_uses(kind)
}

fn successors(t: &Terminator) -> Vec<BlockId> {
    match t {
        Terminator::Jump { target, .. } => vec![*target],
        Terminator::Branch {
            then_blk, else_blk, ..
        } => vec![*then_blk, *else_blk],
        _ => Vec::new(),
    }
}

/// Iterative dominator sets as bitwords: `dom[b]` = blocks dominating `b`.
fn dominators(func: &SsaFunc) -> Vec<Vec<u64>> {
    let n = func.blocks.len();
    let words = n.div_ceil(64);
    let full = vec![u64::MAX; words];
    let mut dom = vec![full; n];

    let entry = func.entry.0 as usize;
    dom[entry] = vec![0; words];
    set_bit(&mut dom[entry], entry);

    let mut changed = true;
    while changed {
        changed = false;
        for b in 0..n {
            if b == entry {
                continue;
            }
            let preds = &func.blocks[b].preds;
            if preds.is_empty() {
                continue;
            }
            let mut new = vec![u64::MAX; words];
            for p in preds {
                for (w, pw) in new.iter_mut().zip(&dom[p.0 as usize]) {
                    *w &= pw;
                }
            }
            set_bit(&mut new, b);
            if new != dom[b] {
                dom[b] = new;
                changed = true;
            }
        }
    }
    dom
}

#[inline]
fn set_bit(bits: &mut [u64], i: usize) {
    bits[i / 64] |= 1 << (i % 64);
}

#[inline]
fn dominates(dom: &[Vec<u64>], a: usize, b: usize) -> bool {
    (dom[b][a / 64] >> (a % 64)) & 1 == 1
}
