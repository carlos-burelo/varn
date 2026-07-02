//! Per-register slot-kind inference.
//!
//! The JIT uses `FunctionProto::register_meta` for two things:
//! * type-specialised codegen (`Int`/`Float`/`Bool` fast paths), and
//! * GC safety: registers whose kind is immediate may skip the flush/reload
//!   around calls (`emit_flush_for_call`), because they can never hold a heap
//!   pointer the collector must see or relocate.
//!
//! A register may be reused for values of different kinds at different program
//! points, and register coalescing (`regalloc_post`) merges live ranges after
//! emission. A kind is therefore only recorded when *every* definition of the
//! register agrees on it; any disagreement or unanalysed definition degrades
//! the register to `Dynamic`, which is always safe.
//!
//! Instruction boundaries come from [`regalloc_post::decode`] — the same
//! decoder the register remapper relies on — so the two passes can never
//! disagree about operand widths.

use varn_core::OpCode;
use varn_types::chunk::{FunctionProto, Literal, PoolEntry};
use varn_types::register_meta::{RegisterMeta, SlotKind};

use crate::regalloc_post::decode;

/// How a single instruction defines its destination register.
enum DefSrc {
    /// The instruction always produces this kind.
    Kind(SlotKind),
    /// `Move`: the destination adopts the source register's kind.
    CopyOf(u8),
    /// Anything else (calls, property loads, globals, …): unknown content.
    Opaque,
}

pub fn infer(proto: &mut FunctionProto) {
    let n = proto.register_count as usize;
    if n == 0 {
        proto.register_meta = Vec::new();
        return;
    }

    let code = &proto.chunk.code;
    let constants = &proto.chunk.constants;

    // None = no definition seen yet (top); Some(Dynamic) = bottom.
    let mut kinds: Vec<Option<SlotKind>> = vec![None; n];

    // Arguments (and `this`) are written by the caller with arbitrary values.
    let fixed = proto.arity + usize::from(proto.has_this);
    for k in kinds.iter_mut().take(fixed.min(n)) {
        *k = Some(SlotKind::Dynamic);
    }

    let mut defs: Vec<(u8, DefSrc)> = Vec::new();

    let mut offset = 0usize;
    while offset < code.len() {
        let Some(info) = decode(code, offset, constants) else {
            // Unknown opcode: the rest of the stream is unanalysable, so no
            // register may claim a skippable kind.
            proto.register_meta = vec![RegisterMeta { kind: SlotKind::Dynamic }; n];
            return;
        };

        if let (Some(dst), Some(op)) = (info.def, OpCode::from_u16(code[offset])) {
            let w1 = code.get(offset + 1).copied().unwrap_or(0);
            let src = classify_def(op, w1, constants);
            defs.push((dst, src));
        }

        offset += info.len;
    }

    // Fixpoint: kinds only descend (unset -> kind -> Dynamic), so this
    // terminates in at most a handful of passes.
    loop {
        let mut changed = false;
        for (dst, src) in &defs {
            let d = *dst as usize;
            if d >= n {
                continue;
            }
            let incoming = match src {
                DefSrc::Kind(k) => Some(*k),
                DefSrc::CopyOf(s) => kinds.get(*s as usize).copied().flatten(),
                DefSrc::Opaque => Some(SlotKind::Dynamic),
            };
            let Some(k) = incoming else { continue };
            let merged = match kinds[d] {
                None => k,
                Some(cur) if cur == k => cur,
                Some(_) => SlotKind::Dynamic,
            };
            if kinds[d] != Some(merged) {
                kinds[d] = Some(merged);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    proto.register_meta = kinds
        .into_iter()
        .map(|k| RegisterMeta {
            kind: k.unwrap_or(SlotKind::Dynamic),
        })
        .collect();
}

fn classify_def(op: OpCode, w1: u16, constants: &[PoolEntry]) -> DefSrc {
    use OpCode::*;
    match op {
        LoadIntZero | LoadIntOne | LoadIntMinusOne | LoadInt => DefSrc::Kind(SlotKind::Int),
        LoadTrue | LoadFalse => DefSrc::Kind(SlotKind::Bool),

        // Int arithmetic that always yields an int (overflow promotes to f64,
        // which is still a non-heap immediate). `DivInt`/`PowInt` are excluded:
        // inexact division yields f64 and pow may overflow, so their result
        // kind is input-dependent.
        AddInt | SubInt | MulInt | ModInt | AddImm | SubImm => DefSrc::Kind(SlotKind::Int),
        StrLength | ArrayLength => DefSrc::Kind(SlotKind::Int),

        AddFloat | SubFloat | MulFloat | DivFloat | ModFloat | PowFloat => {
            DefSrc::Kind(SlotKind::Float)
        }

        Eq | Neq | Lt | Lte | Gt | Gte | EqInt | NeqInt | LtInt | LteInt | GtInt | GteInt
        | EqFloat | NeqFloat | LtFloat | LteFloat | GtFloat | GteFloat | IsNull | IsArray
        | Not | In | Instanceof => DefSrc::Kind(SlotKind::Bool),

        StrConcat | ToString | StrSlice | BuildStr | Typeof => DefSrc::Kind(SlotKind::Str),

        LoadConst => match constants.get(w1 as usize) {
            Some(PoolEntry::Literal(Literal::Int(_))) => DefSrc::Kind(SlotKind::Int),
            Some(PoolEntry::Literal(Literal::Float(_))) => DefSrc::Kind(SlotKind::Float),
            Some(PoolEntry::Literal(Literal::Bool(_))) => DefSrc::Kind(SlotKind::Bool),
            Some(PoolEntry::Literal(Literal::Str(_))) => DefSrc::Kind(SlotKind::Str),
            _ => DefSrc::Opaque,
        },

        Move => DefSrc::CopyOf((w1 >> 8) as u8),

        _ => DefSrc::Opaque,
    }
}
