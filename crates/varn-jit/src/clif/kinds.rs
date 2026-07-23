//! Per-program-point value kinds for the CLIF lowering.
//!
//! Flow-SENSITIVE: the register allocator reuses one register for a
//! comparison result here and an integer there, so kinds are propagated
//! over the bytecode CFG (worklist, merge at joins) and reads validate
//! against the state at the use point. `Poison` marks the call-staging
//! callee slot (`LoadNull`), which the raw path stages but never reads;
//! `Unset` is bottom, `Mixed` top.

use std::collections::HashMap;
use varn_core::OpCode;
use varn_types::bytecode::decode;
use varn_types::register_meta::SlotKind;
use varn_types::VmValue;

#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum K {
    Unset,
    Int,
    Bool,
    /// A NaN-boxed VmValue carried as raw bits (heap refs, non-int params,
    /// untyped loads). Coerces to Int at use via the i48 sign-extend — the
    /// same read the interpreter's typed ops perform.
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
    matches!(k, K::Boxed | K::Global(_))
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
pub(crate) fn apply_kinds(
    state: &mut [K],
    code: &[u16],
    ip: usize,
    op: OpCode,
    constants: &[VmValue],
    meta: &[varn_types::register_meta::RegisterMeta],
) {
    let dest = (code[ip] >> 8) as usize;
    let meta_int = |r: usize| meta.get(r).map_or(false, |m| m.kind == SlotKind::Int);
    match op {
        OpCode::LoadIntZero
        | OpCode::LoadIntOne
        | OpCode::LoadIntMinusOne
        | OpCode::LoadInt
        | OpCode::AddInt
        | OpCode::SubInt
        | OpCode::MulInt
        | OpCode::AddImm
        | OpCode::SubImm
        | OpCode::ModInt
        | OpCode::ArrayLength => state[dest] = K::Int,
        OpCode::LoadTrue | OpCode::LoadFalse => state[dest] = K::Bool,
        // A self-call routes only on an int contract.
        OpCode::CallSelf => {
            let dest = (code[ip + 1] >> 8) as usize;
            state[dest] = K::Int;
        }
        // A static call's result is int on the fast int-contract path, but a
        // generic call (`new`, heap-returning callee) yields a boxed value —
        // let the register meta decide.
        OpCode::Call => {
            let dest = (code[ip + 1] >> 8) as usize;
            state[dest] = if meta_int(dest) { K::Int } else { K::Boxed };
        }
        OpCode::LoadConst => {
            let idx = code[ip + 1] as usize;
            state[dest] = match constants.get(idx) {
                Some(c) if c.is_int() => K::Int,
                // A non-int constant (string, float, null) is carried as its
                // boxed VmValue bits; the lowering embeds them directly.
                Some(_) => K::Boxed,
                None => K::Mixed,
            };
        }
        OpCode::LtInt
        | OpCode::LteInt
        | OpCode::GtInt
        | OpCode::GteInt
        | OpCode::EqInt
        | OpCode::NeqInt
        // Generic comparisons unbox their boxed-bool result to 0/1.
        | OpCode::Lt
        | OpCode::Lte
        | OpCode::Gt
        | OpCode::Gte
        | OpCode::Eq
        | OpCode::Neq
        | OpCode::Not
        | OpCode::IsNull
        | OpCode::Instanceof
        | OpCode::LtFloat
        | OpCode::GtFloat
        | OpCode::LteFloat
        | OpCode::GteFloat
        | OpCode::EqFloat
        | OpCode::NeqFloat => state[dest] = K::Bool,
        OpCode::LoadNull => state[dest] = K::Boxed,
        OpCode::Move => {
            let src = (code[ip + 1] >> 8) as usize;
            state[dest] = state[src];
        }
        // Array element / fixed-field / property loads produce boxed values,
        // unboxed to int when the register meta proves it.
        OpCode::ArrayGetIndex | OpCode::GetFixedField | OpCode::GetProperty => {
            state[dest] = if meta_int(dest) { K::Int } else { K::Boxed };
        }
        // `BuildArray`/`BuildObjectWithShape` encode their destination in the
        // FIRST operand word (`w1 >> 8`), not the opcode word — unlike the
        // ops below whose dest is the standard `first_reg`.
        OpCode::BuildArray | OpCode::BuildObjectWithShape => {
            state[(code[ip + 1] >> 8) as usize] = K::Boxed
        }
        // Other heap-producing ops yield a boxed reference in `first_reg`.
        // A native op / generic Add result kind is unknown here; treat it as
        // boxed bits (an int result still unboxes correctly at an int use).
        OpCode::BuildObject
        | OpCode::StrConcat
        | OpCode::BuildStr
        | OpCode::MakeEnumVariant
        | OpCode::CallNativeOp
        | OpCode::Add
        | OpCode::Sub
        | OpCode::Mul
        | OpCode::Div
        | OpCode::Mod
        | OpCode::Negate
        | OpCode::Typeof
        | OpCode::ToString
        | OpCode::GetSymbol
        | OpCode::AddFloat
        | OpCode::SubFloat
        | OpCode::MulFloat
        | OpCode::DivFloat
        | OpCode::ModFloat
        | OpCode::PowFloat
        | OpCode::Pow
        | OpCode::PowInt => state[dest] = K::Boxed,
        // A global load records its origin so a `Call` on it can link
        // statically; int-typed globals still unbox to Int.
        OpCode::LoadGlobalIdx => {
            state[dest] = if meta_int(dest) {
                K::Int
            } else {
                K::Global(code[ip + 1] as u32)
            };
        }
        _ => {}
    }
}

/// Kind state at every block entry, to a fixpoint over the bytecode CFG.
#[allow(clippy::too_many_arguments)]
pub(crate) fn kind_flow(
    code: &[u16],
    pool: &[varn_types::chunk::PoolEntry],
    constants: &[VmValue],
    block_starts: &[usize],
    nregs: usize,
    param_kinds: &[SlotKind],
    meta: &[varn_types::register_meta::RegisterMeta],
    has_this: bool,
) -> Result<HashMap<usize, Vec<K>>, String> {
    let mut entry0 = vec![K::Unset; nregs];
    // A method/constructor receives its `this` receiver (a heap object) in
    // register 0; every other function leaves r0 as the unused callee slot.
    if has_this && nregs > 0 {
        entry0[0] = K::Boxed;
    }
    for (i, pk) in param_kinds.iter().enumerate() {
        if 1 + i < nregs {
            entry0[1 + i] = if *pk == SlotKind::Int { K::Int } else { K::Boxed };
        }
    }
    let mut entries: HashMap<usize, Vec<K>> = HashMap::new();
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
                    apply_kinds(&mut state, code, ip, op, constants, meta);
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
