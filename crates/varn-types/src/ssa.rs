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
use varn_core::TypeTag;

use crate::register_meta::{SlotClass, SlotKind};

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

/// Typed operation of the scalar/arith family.
///
/// The scalar arithmetic and comparison variants encode the width the checker
/// proved (`IntAdd` vs `FloatAdd`), so a backend never inspects operand types
/// to choose an instruction. `Cast` and `NarrowRangeCheck` are representation
/// facts the checker emitted; both are value-preserving at runtime except the
/// range check, which panics on overflow exactly as the bytecode opcode does.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SsaOp {
    ConstInt(i64),
    ConstFloat(f64),
    ConstBool(bool),
    ConstNull,
    ConstStr(Box<str>),

    Binary {
        op: SsaBinOp,
        lhs: u32,
        rhs: u32,
    },

    Unary {
        op: SsaUnOp,
        operand: u32,
    },

    /// Direct self-recursion. The callee is this same proto, so no linker or
    /// heap reference is involved — only the call arguments, in order.
    SelfCall {
        args: Vec<u32>,
    },

    /// Cross-function call. `callee` is a `Ref`/`Dyn` value (a closure) and
    /// `callee_global` is the module-relative global slot it was loaded from,
    /// when known — what the linker resolves a static target by. The fallback
    /// is always the canonical `ExecCtx::invoke`.
    Call {
        callee: u32,
        callee_global: Option<u32>,
        args: Vec<u32>,
    },

    /// Module-relative global read (`GlobalStore[closure.module_base + slot]`),
    /// a boxed `VmValue` (`Ref`/`Dyn`).
    LoadGlobalIdx(u32),

    /// Checker-proven cast. Width-narrowing casts already carry a
    /// [`SsaOp::NarrowRangeCheck`]; a `Cast` itself is representation-neutral.
    Cast { operand: u32 },
    /// Numeric conversion (`as`) that changes representation.
    Convert { operand: u32, conv: varn_core::NumConv },

    /// Validate `operand` fits `tag`'s range, passing it through unchanged.
    NarrowRangeCheck { operand: u32, tag: TypeTag },

    IsNull { operand: u32 },

    /// `typeof x` — a heap string result.
    Typeof { operand: u32 },

    /// `String(x)` — a heap string result.
    ToString { operand: u32 },

    /// Runtime array test; a `bool` result.
    IsArray { operand: u32 },

    /// Enum discriminant of `operand`; an `int` result.
    GetEnumTag { operand: u32 },

    /// Own enumerable keys of `operand`; a heap array result.
    ObjectKeys { operand: u32 },

    /// Interpolated string from `parts`; a heap string result.
    BuildStr { parts: Vec<u32> },

    /// Array literal from `elements`; a heap array result.
    BuildArray { elements: Vec<u32> },

    /// Map literal from `pairs`; a heap map result.
    BuildMap { pairs: Vec<(u32, u32)> },

    /// Object/record literal from `keys`/`values`; a heap result. The shape is
    /// resolved from the proto's pool by key match at lowering time.
    BuildObject {
        keys: Vec<Box<str>>,
        values: Vec<u32>,
        is_record: bool,
    },

    /// Dynamic property read; `cs` is the inline-cache slot. A heap result.
    GetProperty {
        object: u32,
        name: Box<str>,
        cs: u16,
    },

    /// Dynamic property write; no result.
    SetProperty {
        object: u32,
        value: u32,
        name: Box<str>,
        cs: u16,
    },

    /// `obj[index]` — a heap result.
    GetIndex { object: u32, index: u32 },

    /// `obj[index] = value` — no result.
    SetIndex { object: u32, index: u32, value: u32 },

    /// `arr.length` — an `int` result.
    ArrayLength { operand: u32 },

    /// `arr.push(value)` — no result.
    ArrayPush { array: u32, value: u32 },

    /// The current receiver (`this`), read from home 0; a heap result.
    This,

    /// Class/object field read by dynamic `slot`; a heap result.
    GetFixedField { object: u32, slot: u16 },

    /// Class/object field write by dynamic `slot`; no result.
    SetFixedField { object: u32, value: u32, slot: u16 },

    /// `class Name [extends Super]` — a heap class object.
    MakeClass {
        name: Box<str>,
        super_class: Option<u32>,
    },

    /// `DeclareField` on a class; no result.
    DeclareField {
        class: u32,
        name: Box<str>,
        tag: TypeTag,
    },

    /// A class member definition (`Method`/`DefineStatic`/accessors). `kind` is
    /// the runtime discriminant: Method=0, DefineStatic=1, DefineGetter=2,
    /// DefineSetter=3, DefineStaticGetter=4, DefineStaticSetter=5.
    DefineMethod {
        class: u32,
        name: Box<str>,
        member: u32,
        kind: u8,
    },

    /// `GetSuper name` — a heap result.
    GetSuper { name: Box<str> },
}

/// Binary operations, already specialized to a physical domain.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SsaBinOp {
    IntAdd,
    IntSub,
    IntMul,
    IntDiv,
    IntMod,
    IntPow,
    IntEq,
    IntNe,
    IntLt,
    IntLe,
    IntGt,
    IntGe,
    IntAnd,
    IntOr,
    IntXor,
    IntShl,
    IntShr,
    IntUshr,

    FloatAdd,
    FloatSub,
    FloatMul,
    FloatDiv,
    FloatMod,
    FloatPow,
    FloatEq,
    FloatNe,
    FloatLt,
    FloatLe,
    FloatGt,
    FloatGe,

    /// Statically-proven string concatenation (`"a" + b`).
    StrConcat,
}

/// Unary operations, specialized to a physical domain.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SsaUnOp {
    NegInt,
    NegFloat,
    /// Logical negation; result is a bool (`Dyn` class).
    Not,
    BitNotInt,
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
                        SsaInst {
                            dest: Some(8),
                            op: SsaOp::NarrowRangeCheck {
                                operand: 7,
                                tag: TypeTag::I32,
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
