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
    pub const MODULE: &'static str = "std:math";

    const ALL: [Self; 13] = [
        Self::Abs,
        Self::Sqrt,
        Self::Floor,
        Self::Ceil,
        Self::Round,
        Self::Sin,
        Self::Cos,
        Self::Tan,
        Self::Log,
        Self::Exp,
        Self::Pow,
        Self::Min,
        Self::Max,
    ];

    pub const fn wire(self) -> u8 {
        self as u8
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Abs => "abs",
            Self::Sqrt => "sqrt",
            Self::Floor => "floor",
            Self::Ceil => "ceil",
            Self::Round => "round",
            Self::Sin => "sin",
            Self::Cos => "cos",
            Self::Tan => "tan",
            Self::Log => "log",
            Self::Exp => "exp",
            Self::Pow => "pow",
            Self::Min => "min",
            Self::Max => "max",
        }
    }

    pub fn of(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|op| op.name() == name)
    }

    pub fn of_wire(wire: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|op| op.wire() == wire)
    }
}

pub fn is_unary_math(wire_byte: u8) -> bool {
    let op = wire_byte;
    op == MathOp::Abs as u8
        || op == MathOp::Sqrt as u8
        || op == MathOp::Floor as u8
        || op == MathOp::Ceil as u8
}
