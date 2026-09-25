//! Unary and binary operators of [`super::SsaOp`], each already specialized
//! to the physical domain the checker proved.

use serde::{Deserialize, Serialize};

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

    /// The operator on boxed operands, run by its runtime helper: the
    /// bytecode's generic opcode, for operands no type proves native.
    Dyn(DynBinOp),
}

/// A binary operator on boxed values: arithmetic and bitwise ones yield a
/// boxed value, comparisons, `instanceof` and `in` a `bool`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DynBinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    Ushr,
    Instanceof,
    In,
}

/// A unary operator on a boxed value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DynUnOp {
    /// `-x`; a boxed result.
    Neg,
    /// `!x` by truthiness; a `bool`.
    Not,
    /// `~x`; a boxed result.
    BitNot,
}

/// Unary operations, specialized to a physical domain.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SsaUnOp {
    NegInt,
    NegFloat,
    /// Logical negation; result is a bool (`Dyn` class).
    Not,
    BitNotInt,
    /// The operator on the boxed operand, run by its runtime helper.
    Dyn(DynUnOp),
}
