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
    /// An unboxed `f64` in an `F64` Cranelift Variable. Static per register:
    /// a register is `Float` iff `register_meta[r].kind == SlotKind::Float`,
    /// seeded at entry and preserved by the flow (see `clif::floats`).
    Float,
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
    matches!(k, K::Boxed | K::Global(_) | K::Mixed)
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
    constants: &[VmValue],
    meta: &[varn_types::register_meta::RegisterMeta],
    return_kind: SlotKind,
) {
    let dest = match decode(code, ip, pool).and_then(|i| i.def) {
        Some(d) if (d as usize) < state.len() => d as usize,
        _ => return,
    };
    let meta_int = |r: usize| meta.get(r).is_some_and(|m| m.kind == SlotKind::Int);
    let meta_float = |r: usize| meta.get(r).is_some_and(|m| m.kind == SlotKind::Float);
    // A result the emitter serves in the register's declared representation
    // (it unboxes to int / converts to f64 when the meta proves the type).
    let typed = |r: usize| {
        if meta_float(r) {
            K::Float
        } else if meta_int(r) {
            K::Int
        } else {
            K::Boxed
        }
    };
    // A result the emitter always produces as boxed VmValue bits. A float
    // register still holds an unboxed `f64` (its Variable is `F64`), because
    // the emitters convert on the way into the var.
    let boxed = |r: usize| if meta_float(r) { K::Float } else { K::Boxed };
    match op {
        OpCode::AddInt
        | OpCode::SubInt
        | OpCode::MulInt
        | OpCode::AddImm
        | OpCode::SubImm
        | OpCode::ModInt
        | OpCode::ArrayLength => state[dest] = K::Int,
        // An int literal into a float-typed sink loads as an unboxed f64
        // (`def_const_int` branches on the same meta).
        OpCode::LoadIntZero
        | OpCode::LoadIntOne
        | OpCode::LoadIntMinusOne
        | OpCode::LoadInt => state[dest] = if meta_float(dest) { K::Float } else { K::Int },
        OpCode::LoadTrue | OpCode::LoadFalse => state[dest] = K::Bool,
        // A self-call returns in THIS function's own return convention, so its
        // result kind is `return_kind`, never a constant.
        //
        // `emit_return_value` produces a raw, unboxed i48 for a `SlotKind::Int`
        // return and BOXED VmValue bits for every other kind. This arm used to
        // say `K::Int` unconditionally — correct only for the int contract, and
        // silently wrong for the rest: `box_or_pass` then ran `box_int` over
        // boxed bits, replacing the value's tag with the int tag while keeping
        // its low 48 bits. For a heap value those low bits are the heap INDEX,
        // so a recursive function returning `str` handed its caller the slot
        // number as an integer:
        //
        //     function repeatStr(s: str, n: int): str {
        //         if (n <= 0) { return "" }
        //         return s + repeatStr(s, n - 1)   // "ab" + 52
        //     }
        //
        OpCode::CallSelf => {
            state[dest] = if meta_float(dest) || return_kind == SlotKind::Float {
                K::Float
            } else if return_kind == SlotKind::Int {
                K::Int
            } else if return_kind == SlotKind::Bool {
                K::Bool
            } else {
                boxed(dest)
            }
        }
        // A call result is ALWAYS boxed bits, even into an `int`-typed slot:
        // the register meta types the slot, not the value, and stdlib code
        // relies on the VM coercing a whole float (`int_div`) at the int sink
        // rather than at the definition. Unboxing here would read those float
        // bits as an i48 payload. The fast int-contract IC re-boxes its raw
        // result to keep this one representation (see `lower`'s `Call` arm).
        OpCode::Call | OpCode::CallSpread => state[dest] = boxed(dest),
        OpCode::LoadConst => {
            let idx = code[ip + 1] as usize;
            state[dest] = if meta_float(dest) {
                // A float-typed sink: the constant loads as an unboxed f64
                // (a float literal, or an int literal widened to float).
                K::Float
            } else {
                match constants.get(idx) {
                    Some(c) if c.is_int() => K::Int,
                    // A non-int constant (string, float, null) is carried as its
                    // boxed VmValue bits; the lowering embeds them directly.
                    Some(_) => K::Boxed,
                    None => K::Mixed,
                }
            };
        }
        // Typed float arithmetic yields an unboxed f64 in a Float register; a
        // non-float (untyped) sink keeps boxed bits and routes to the helper.
        OpCode::AddFloat
        | OpCode::SubFloat
        | OpCode::MulFloat
        | OpCode::DivFloat
        | OpCode::ModFloat
        | OpCode::PowFloat => {
            state[dest] = if meta_float(dest) { K::Float } else { K::Boxed };
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
        | OpCode::In
        | OpCode::IsArray
        | OpCode::LtFloat
        | OpCode::GtFloat
        | OpCode::LteFloat
        | OpCode::GteFloat
        | OpCode::EqFloat
        | OpCode::NeqFloat => state[dest] = K::Bool,
        OpCode::LoadNull => state[dest] = K::Boxed,
        // The lowering converts across representations (int→f64 into a float
        // sink, f64→boxed into a non-float one), so the kind follows the
        // DESTINATION's declared representation, not the source's.
        OpCode::Move => {
            let src = (code[ip + 1] >> 8) as usize;
            state[dest] = state[src];
        }
        // Results the emitter serves in the register's DECLARED
        // representation: `clif::arrays` and `clif::fields` convert/unbox to the
        // wanted repr (an `I64` into an `Int` register, an `F64` into an `F64` register).
        OpCode::ArrayGetIndex | OpCode::GetFixedField => state[dest] = typed(dest),
        // Everything else helper-backed lands as boxed VmValue bits (a float
        // register still holds an unboxed `f64` — the emitters coerce on the
        // way in). Claiming `Int` here because the register meta types the
        // SLOT as int is a lie about the VALUE: `int_div` returns a whole
        // float into an `int` slot, and reading those bits as an i48 payload
        // yields garbage. Int consumers unbox at the use instead.
        OpCode::GetProperty
        | OpCode::BuildArray
        | OpCode::BuildTuple
        | OpCode::BuildObjectWithShape
        | OpCode::BuildRecord
        | OpCode::CallMethod
        | OpCode::InvokeVirtual
        | OpCode::BuildObject
        | OpCode::StrConcat
        | OpCode::BuildStr
        | OpCode::MakeEnumVariant
        | OpCode::CallNativeOp
        | OpCode::GetEnumTag
        | OpCode::Add
        | OpCode::Sub
        | OpCode::Mul
        | OpCode::Div
        | OpCode::DivInt
        | OpCode::Mod => state[dest] = boxed(dest),
        OpCode::Negate => {
            let src = (code[ip + 1] >> 8) as usize;
            if meta_float(dest) || state[src] == K::Float {
                state[dest] = K::Float;
            } else if meta_int(dest) || state[src] == K::Int {
                state[dest] = K::Int;
            } else {
                state[dest] = boxed(dest);
            }
        }
        OpCode::BitAnd
        | OpCode::BitOr
        | OpCode::BitXor
        | OpCode::Shl
        | OpCode::Shr
        | OpCode::Ushr => {
            if meta_int(dest) {
                state[dest] = K::Int;
            } else {
                state[dest] = boxed(dest);
            }
        }
        OpCode::Intrinsic
        | OpCode::IntrinsicDirect
        | OpCode::Typeof
        | OpCode::ToString
        | OpCode::GetSymbol
        | OpCode::Pow
        | OpCode::PowInt
        | OpCode::StrSlice
        | OpCode::StrLength
        | OpCode::ArrayPop
        | OpCode::GetIndex
        | OpCode::GetPropertyMaybe
        | OpCode::BindMethod
        | OpCode::ObjectKeys
        | OpCode::ObjectMerge
        | OpCode::ObjectRest
        | OpCode::WrapSpread
        | OpCode::ArrayExtend
        | OpCode::MakeClass
        | OpCode::GetSuper
        // `resolve_in_proto` is meant to rewrite every one of these into
        // `LoadGlobalIdx`, but the fallthrough for an unlisted opcode LEAVES
        // the destination's kind alone, so a survivor inherits whatever the
        // register last held — an `Int` param, and then the callee is passed
        // as a raw i64. Listing it costs nothing when the rewrite did happen.
        | OpCode::LoadGlobal
        | OpCode::LoadUpvalue
        | OpCode::MakeClosure
        | OpCode::LoadStaticFn
        | OpCode::LoadModule
        | OpCode::LoadModuleSlot
        | OpCode::InvokeRuntimeStatic
        | OpCode::Await
        | OpCode::Spawn
        // `Try` defines the catch handler's error register. The value lands
        // there on the exception path (which leaves clif code entirely), but
        // the register is boxed for every reader downstream.
        | OpCode::Try => state[dest] = boxed(dest),
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
    return_kind: SlotKind,
) -> Result<HashMap<usize, Vec<K>>, String> {
    let mut entry0 = vec![K::Unset; nregs];
    // A method/constructor receives its `this` receiver (a heap object) in
    // register 0; every other function leaves r0 as the unused callee slot.
    if has_this && nregs > 0 {
        entry0[0] = K::Boxed;
    }
    for (i, pk) in param_kinds.iter().enumerate() {
        let r = 1 + i;
        if r < nregs {
            entry0[r] = match *pk {
                SlotKind::Int => K::Int,
                SlotKind::Bool => K::Bool,
                SlotKind::Float => K::Float,
                _ => K::Boxed,
            };
        }
    }
    // A float-typed register is an F64 Variable for the whole function (static
    // representation), so seed it `Float` at entry — params (overriding the
    // boxed default above) and locals alike; the flow preserves it.
    for (r, e) in entry0.iter_mut().enumerate() {
        if meta.get(r).is_some_and(|m| m.kind == SlotKind::Float) {
            *e = K::Float;
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
                    apply_kinds(&mut state, code, pool, ip, op, constants, meta, return_kind);
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
