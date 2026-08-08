use super::wire::{encode, IntrinsicDomain};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MathOp {
    Abs = 0x0,
    Sqrt = 0x1,
    Floor = 0x2,
    Ceil = 0x3,
    Round = 0x4,
    Sin = 0x5,
    Cos = 0x6,
    Tan = 0x7,
    Log = 0x8,
    Exp = 0x9,
    Pow = 0xA,
    Min = 0xB,
    Max = 0xC,
}

impl MathOp {
    pub const fn wire(self) -> u8 {
        encode(IntrinsicDomain::Math, self as u8)
    }
}

/// Whether `wire_byte` names a math op that [`crate::OpCode::IntrinsicDirect`]
/// carries without a call window.
///
/// Deliberately narrower than "unary": only the four ops the JIT can lower to
/// a single IEEE instruction. The direct form has no window for a helper to
/// read arguments out of, so an op emitted here that the JIT cannot lower
/// natively would have nowhere to fall back TO. `round`/`sin`/`log`/… stay on
/// the windowed `Intrinsic`, which reaches the generic helper as before.
pub fn is_unary_math(wire_byte: u8) -> bool {
    let (domain, op) = super::wire::decode(wire_byte);
    if domain != IntrinsicDomain::Math as u8 {
        return false;
    }
    op == MathOp::Abs as u8
        || op == MathOp::Sqrt as u8
        || op == MathOp::Floor as u8
        || op == MathOp::Ceil as u8
}

pub const MAP_ENTRIES: &[(&str, u8)] = &[
    ("std:math/abs", MathOp::Abs.wire()),
    ("std:math/sqrt", MathOp::Sqrt.wire()),
    ("std:math/floor", MathOp::Floor.wire()),
    ("std:math/ceil", MathOp::Ceil.wire()),
    ("std:math/round", MathOp::Round.wire()),
    ("std:math/sin", MathOp::Sin.wire()),
    ("std:math/cos", MathOp::Cos.wire()),
    ("std:math/tan", MathOp::Tan.wire()),
    ("std:math/log", MathOp::Log.wire()),
    ("std:math/exp", MathOp::Exp.wire()),
    ("std:math/pow", MathOp::Pow.wire()),
    ("std:math/min", MathOp::Min.wire()),
    ("std:math/max", MathOp::Max.wire()),
];
