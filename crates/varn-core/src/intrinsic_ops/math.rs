use super::wire::{encode, IntrinsicDomain};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MathOp {
    Abs   = 0x0,
    Sqrt  = 0x1,
    Floor = 0x2,
    Ceil  = 0x3,
    Round = 0x4,
    Sin   = 0x5,
    Cos   = 0x6,
    Tan   = 0x7,
    Log   = 0x8,
    Exp   = 0x9,
    Pow   = 0xA,
    Min   = 0xB,
    Max   = 0xC,
}

impl MathOp {
    pub const fn wire(self) -> u8 {
        encode(IntrinsicDomain::Math, self as u8)
    }
}

pub const MAP_ENTRIES: &[(&str, u8)] = &[
    ("std:math/abs",   MathOp::Abs.wire()),
    ("std:math/sqrt",  MathOp::Sqrt.wire()),
    ("std:math/floor", MathOp::Floor.wire()),
    ("std:math/ceil",  MathOp::Ceil.wire()),
    ("std:math/round", MathOp::Round.wire()),
    ("std:math/sin",   MathOp::Sin.wire()),
    ("std:math/cos",   MathOp::Cos.wire()),
    ("std:math/tan",   MathOp::Tan.wire()),
    ("std:math/log",   MathOp::Log.wire()),
    ("std:math/exp",   MathOp::Exp.wire()),
    ("std:math/pow",   MathOp::Pow.wire()),
    ("std:math/min",   MathOp::Min.wire()),
    ("std:math/max",   MathOp::Max.wire()),
];
