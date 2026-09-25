//! Portable, serializable typed SSA: the contract the JIT lowers from.
//!
//! Today the JIT consumes bytecode and rebuilds the type of every register
//! with a flow lattice over `register_meta`. That reconstruction is a second
//! source of truth for "what is this value" and it can disagree with the
//! checker (the value-flow bugs C1 chased: a `str` arriving as `heap[46]`, a
//! `Bool` branch on a raw pair). This module carries the OTHER source: each
//! value's physical class, decided once by the compiler, with the same
//! typed operations it already emits as opcodes.
//!
//! Design rules (Ley 2/6/8):
//!
//! * **No internal ids cross the boundary.** Values are dense `u32` indices
//!   into [`SsaProto::values`]; there is no `HirType`, `CheckerTyId` or
//!   `LocalId`. The physical projection is [`SsaTy`] = [`SlotClass`], the very
//!   same `(Gpr/Fpr/Ref/Dyn)` both execution tiers already lower against.
//! * **One fact, carried once.** Whether a `+` is integer or float is a
//!   [`SsaBinOp`] variant, not something the backend re-derives from operand
//!   types. `Serde`-stable and `postcard`-friendly (no `Arc`, no `Rc`, no
//!   cell): the artifact must round-trip through `.vnc` unchanged.
//! * **Extensible by family.** Only the scalar/arith family is modelled here;
//!   a function whose body uses anything else simply does not get an
//!   [`SsaProto`] and keeps the bytecode lowering (the fallback is a missing
//!   SSA, never a wrong one).
//!
//! A proto is attached **after regalloc**, so [`SsaProto::regs`] maps every
//! value to the VM register the interpreter holds it in; a lowering that keeps
//! values in native registers can ignore it, a lowering that must spill to the
//! frame (calls, safepoints) reads the same coordinates the interpreter does.

use serde::{Deserialize, Serialize};

use crate::register_meta::{SlotClass, SlotKind};

mod op;
mod operators;

pub use op::{SsaOp, SsaUpvalue, UPVALUE_LOCAL};
pub use operators::{DynBinOp, DynUnOp, SsaBinOp, SsaUnOp};

/// The static kind of a value, exactly the checker's proof projected onto the
/// register file. It is the same [`SlotKind`] the JIT already reads from
/// `register_meta` — but *per value* rather than met across a register's whole
/// live range, so two disjoint values sharing a register can no longer erase
/// each other's type. [`SlotKind::class`] is the physical projection
/// (`Gpr/Fpr/Ref/Dyn`) both tiers lower against (Ley 6).
pub type SsaTy = SlotKind;

/// A typed SSA value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SsaValue {
    pub ty: SsaTy,
}

impl SsaValue {
    /// Physical storage class of this value.
    #[inline]
    pub fn class(&self) -> SlotClass {
        SlotClass::of_kind(self.ty)
    }
}

/// One basic block. `params` are the block's phi values (filled by each
/// predecessor's terminator args); `insts` define new values; `term` exits.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SsaBlock {
    pub params: Vec<u32>,
    pub insts: Vec<SsaInst>,
    pub term: SsaTerm,
}

/// A defining instruction. `dest` is the value it defines, if any.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SsaInst {
    pub dest: Option<u32>,
    pub op: SsaOp,
    pub line: u32,
}

/// A terminator. Jump/branch args fill the target block's `params`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SsaTerm {
    Return(Option<u32>),
    Throw(u32),
    Jump {
        target: u32,
        args: Vec<u32>,
    },
    Branch {
        cond: u32,
        then_blk: u32,
        then_args: Vec<u32>,
        else_blk: u32,
        else_args: Vec<u32>,
    },
    Unreachable,
}

/// A whole function's portable typed SSA.
///
/// `regs[v]` is the VM register value `v` resides in after regalloc;
/// `register_count` is the frame size both tiers agree on. The mapping is
/// redundant with the bytecode on purpose — it is what lets the JIT place a
/// value without re-walking the opcode stream.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SsaProto {
    pub name: Box<str>,
    pub nparams: u32,
    pub entry: u32,
    pub blocks: Vec<SsaBlock>,
    pub values: Vec<SsaValue>,
    pub regs: Vec<u32>,
    pub register_count: u16,
    pub has_this: bool,
    /// The frame register of each captured variable, by the index the
    /// closure ops name it with.
    pub captured: Vec<u32>,
}

impl SsaProto {
    #[inline]
    pub fn block(&self, id: u32) -> &SsaBlock {
        &self.blocks[id as usize]
    }

    #[inline]
    pub fn value_ty(&self, v: u32) -> SsaTy {
        self.values[v as usize].ty
    }

    #[inline]
    pub fn reg(&self, v: u32) -> u32 {
        self.regs.get(v as usize).copied().unwrap_or(0)
    }

    /// The frame register of captured variable `var`.
    #[inline]
    pub fn captured_reg(&self, var: u32) -> Option<u32> {
        self.captured.get(var as usize).copied()
    }

    /// Renumber every register this SSA names — the values' homes and the
    /// captured variables — through `f`, as a pass that renumbers the
    /// bytecode's registers must. The one place a register field is listed.
    pub fn map_registers(&mut self, f: impl Fn(u32) -> u32) {
        for r in self.regs.iter_mut().chain(self.captured.iter_mut()) {
            *r = f(*r);
        }
    }
}

/// A function's portable SSA, or why it has none. The JIT lowers a function
/// without one from bytecode, and `VARN_CLIF_TRACE` reports the reason — a
/// function never falls back silently.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum PortableSsa {
    Available(std::sync::Arc<SsaProto>),
    Unavailable(std::sync::Arc<str>),
}

impl Default for PortableSsa {
    fn default() -> Self {
        Self::Unavailable(std::sync::Arc::from("not built from typed SSA"))
    }
}

impl PortableSsa {
    pub fn get(&self) -> Option<&SsaProto> {
        match self {
            Self::Available(ssa) => Some(ssa),
            Self::Unavailable(_) => None,
        }
    }

    pub fn get_mut(&mut self) -> Option<&mut SsaProto> {
        match self {
            Self::Available(ssa) => Some(std::sync::Arc::make_mut(ssa)),
            Self::Unavailable(_) => None,
        }
    }

    /// Why there is no portable SSA, when there is none.
    pub fn unavailable(&self) -> Option<&str> {
        match self {
            Self::Available(_) => None,
            Self::Unavailable(why) => Some(why),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> SsaProto {
        SsaProto {
            name: "sum_squares".into(),
            nparams: 1,
            entry: 0,
            blocks: vec![
                SsaBlock {
                    params: vec![0],
                    insts: vec![
                        SsaInst {
                            dest: Some(1),
                            op: SsaOp::ConstInt(0),
                            line: 1,
                        },
                        SsaInst {
                            dest: Some(2),
                            op: SsaOp::ConstInt(1),
                            line: 1,
                        },
                    ],
                    term: SsaTerm::Jump {
                        target: 1,
                        args: vec![1, 2],
                    },
                },
                SsaBlock {
                    params: vec![3, 4],
                    insts: vec![
                        SsaInst {
                            dest: Some(5),
                            op: SsaOp::Binary {
                                op: SsaBinOp::IntLe,
                                lhs: 3,
                                rhs: 0,
                            },
                            line: 2,
                        },
                        SsaInst {
                            dest: Some(6),
                            op: SsaOp::Binary {
                                op: SsaBinOp::IntAdd,
                                lhs: 4,
                                rhs: 3,
                            },
                            line: 3,
                        },
                        SsaInst {
                            dest: Some(7),
                            op: SsaOp::Unary {
                                op: SsaUnOp::NegInt,
                                operand: 6,
                            },
                            line: 3,
                        },
                    ],
                    term: SsaTerm::Branch {
                        cond: 5,
                        then_blk: 2,
                        then_args: vec![],
                        else_blk: 3,
                        else_args: vec![6, 4],
                    },
                },
                SsaBlock {
                    params: vec![],
                    insts: vec![],
                    term: SsaTerm::Return(Some(4)),
                },
                SsaBlock {
                    params: vec![9, 10],
                    insts: vec![SsaInst {
                        dest: Some(11),
                        op: SsaOp::ConstFloat(0.5),
                        line: 4,
                    }],
                    term: SsaTerm::Jump {
                        target: 1,
                        args: vec![],
                    },
                },
            ],
            values: vec![
                SsaValue { ty: SsaTy::Int },
                SsaValue { ty: SsaTy::Int },
                SsaValue { ty: SsaTy::Int },
                SsaValue { ty: SsaTy::Int },
                SsaValue { ty: SsaTy::Int },
                SsaValue { ty: SsaTy::Bool },
                SsaValue { ty: SsaTy::Int },
                SsaValue { ty: SsaTy::Int },
                SsaValue { ty: SsaTy::Int },
                SsaValue { ty: SsaTy::Int },
                SsaValue { ty: SsaTy::Int },
                SsaValue { ty: SsaTy::Float },
            ],
            regs: vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12],
            register_count: 13,
            has_this: false,
            captured: Vec::new(),
        }
    }

    #[test]
    fn ssa_proto_round_trips_through_postcard() {
        let proto = sample();
        let bytes = postcard::to_allocvec(&proto).expect("serialize");
        let back: SsaProto = postcard::from_bytes(&bytes).expect("deserialize");
        assert_eq!(proto, back);
    }
}
