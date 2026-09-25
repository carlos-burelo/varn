//! Per-program-point value kinds for the CLIF lowering.
//!
//! Flow-SENSITIVE: the register allocator reuses one register for a
//! comparison result here and an integer there, so kinds are propagated
//! over the bytecode CFG (worklist, merge at joins) and reads validate
//! against the state at the use point. `Poison` marks the call-staging
//! callee slot (`LoadNull`), which the raw path stages but never reads;
//! `Unset` is bottom, `Mixed` top.

use rustc_hash::FxHashMap as HashMap;
use varn_core::OpCode;
use varn_types::bytecode::decode;
use varn_types::register_meta::SlotKind;
use varn_types::VmValue;

#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum K {
    Unset,
    Int,
    /// An unboxed `f64` in an `F64` Cranelift Variable. Static per register:
    /// a register is `Float` iff `register_meta[r].kind == SlotKind::Float`,
    /// seeded at entry and preserved by the flow (see `clif::floats`).
    Float,
    Bool,
    /// A tag+payload `VmValue` pair (heap refs, non-int params, untyped
    /// loads). `unbox_int` at use extracts the payload word directly.
    Boxed,
    /// A value freshly loaded from global slot `idx` — a `Boxed` refinement
    /// that additionally records the origin, so a `Call` on it can ask the
    /// linker for a static target. Any reuse as a plain boxed value treats
    /// it as `Boxed`; two different origins meet to `Boxed`.
    Global(u32),
    Poison,
    Mixed,
}

/// Whether `k` can be read as a boxed VmValue (heap receiver / call arg).
pub(crate) fn is_boxed_kind(k: K) -> bool {
    matches!(k, K::Boxed | K::Global(_) | K::Mixed)
}

/// The kind a register holds, projected from its physical CLASS
/// (`register_meta`). This is C4's replacement for the flow analysis: after C1
/// every producer writes its destination in the destination's declared class,
/// so the kind at any use is a property of the register, not of the program
/// point. `Bool`/`Str` are `SlotClass::Dyn` (a pair), hence `Boxed`.
pub(crate) fn class_kind(meta: &[varn_types::register_meta::RegisterMeta], r: usize) -> K {
    use varn_types::register_meta::SlotClass;
    match meta
        .get(r)
        .map(|m| SlotClass::of_kind(m.kind))
        .unwrap_or(SlotClass::Dyn)
    {
        SlotClass::Fpr => K::Float,
        SlotClass::Gpr => K::Int,
        SlotClass::Ref | SlotClass::Dyn => K::Boxed,
    }
}

fn merge(cur: K, k: K) -> K {
    match (cur, k) {
        (K::Unset, x) | (x, K::Unset) => x,
        (a, b) if a == b => a,
        // Two boxed-ish kinds (incl. distinct global origins) stay boxed.
        (a, b) if is_boxed_kind(a) && is_boxed_kind(b) => K::Boxed,
        _ => K::Mixed,
    }
}

/// Apply one instruction's effect on the kind state. Shared by the dataflow
/// pass and the lowering walk so the two can never disagree.
///
/// The destination register comes from [`decode`] — the single authority on
/// instruction shape — never from a per-opcode guess about which word carries
/// it. Every opcode the lowering routes must be classified here: an opcode
/// that defines a register but is missing from the match leaves the dataflow
/// believing the old kind, and the lowering then reads the register with the
/// wrong representation at the next join.
pub(crate) fn apply_kinds(
    state: &mut [K],
    code: &[u16],
    pool: &[varn_types::chunk::PoolEntry],
    ip: usize,
    op: OpCode,
    _constants: &[VmValue],
    meta: &[varn_types::register_meta::RegisterMeta],
    _return_kind: SlotKind,
) {
    let dest = match decode(code, ip, pool).and_then(|i| i.def) {
        Some(d) if (d as usize) < state.len() => d as usize,
        _ => return,
    };
    // C4: the kind at a use is the destination's CLASS projection, not a
    // re-derivation from the op. The one flow-proven fact worth keeping is a
    // global load's ORIGIN — `Call` asks the linker for a static target from
    // it, and that is a property of where the value came from, not of its
    // representation (which the class already fixes).
    state[dest] = match op {
        OpCode::LoadGlobalIdx => {
            if meta.get(dest).is_some_and(|m| m.kind == SlotKind::Int) {
                K::Int
            } else {
                K::Global(code[ip + 1] as u32)
            }
        }
        _ => class_kind(meta, dest),
    };
}

/// Kind state at every block entry, to a fixpoint over the bytecode CFG.
#[allow(clippy::too_many_arguments)]
pub(crate) fn kind_flow(
    code: &[u16],
    pool: &[varn_types::chunk::PoolEntry],
    constants: &[VmValue],
    block_starts: &[usize],
    nregs: usize,
    _param_kinds: &[SlotKind],
    meta: &[varn_types::register_meta::RegisterMeta],
    has_this: bool,
    _return_kind: SlotKind,
) -> Result<HashMap<usize, Vec<K>>, String> {
    // C4: every block's entry state is the class projection. There is no flow
    // to a fixpoint — the kind is a property of the register, not the point.
    let mut entry0 = vec![K::Unset; nregs];
    for (r, e) in entry0.iter_mut().enumerate() {
        *e = class_kind(meta, r);
    }
    if has_this && nregs > 0 {
        entry0[0] = K::Boxed;
    }
    let mut entries: HashMap<usize, Vec<K>> = HashMap::default();
    entries.insert(0, entry0);
    let mut work: Vec<usize> = vec![0];

    while let Some(start) = work.pop() {
        let mut state = entries[&start].clone();
        let mut ip = start;
        loop {
            if ip >= code.len() {
                break;
            }
            if ip != start && block_starts.contains(&ip) {
                // fall-through edge into the next block
                propagate(&mut entries, &mut work, ip, &state);
                break;
            }
            let info = decode(code, ip, pool).ok_or("clif: undecodable opcode")?;
            let op = OpCode::from_u8(code[ip] as u8).ok_or("clif: unknown opcode")?;
            match op {
                OpCode::Jump => {
                    let off = ((code[ip + 1] as u32) << 16 | code[ip + 2] as u32) as usize;
                    propagate(&mut entries, &mut work, ip + 3 + off, &state);
                    break;
                }
                OpCode::Loop => {
                    let off = ((code[ip + 1] as u32) << 16 | code[ip + 2] as u32) as usize;
                    propagate(&mut entries, &mut work, (ip + 3) - off, &state);
                    break;
                }
                OpCode::JumpIfFalse | OpCode::JumpIfTrue => {
                    let off = ((code[ip + 1] as u32) << 16 | code[ip + 2] as u32) as usize;
                    propagate(&mut entries, &mut work, ip + 3 + off, &state);
                    propagate(&mut entries, &mut work, ip + 3, &state);
                    break;
                }
                OpCode::Return => break,
                _ => {
                    apply_kinds(
                        &mut state,
                        code,
                        pool,
                        ip,
                        op,
                        constants,
                        meta,
                        _return_kind,
                    );
                }
            }
            ip += info.len;
        }
    }
    Ok(entries)
}

fn propagate(
    entries: &mut HashMap<usize, Vec<K>>,
    work: &mut Vec<usize>,
    target: usize,
    state: &[K],
) {
    match entries.get_mut(&target) {
        Some(cur) => {
            let mut changed = false;
            for (c, s) in cur.iter_mut().zip(state) {
                let m = merge(*c, *s);
                if m != *c {
                    *c = m;
                    changed = true;
                }
            }
            if changed {
                work.push(target);
            }
        }
        None => {
            entries.insert(target, state.to_vec());
            work.push(target);
        }
    }
}
