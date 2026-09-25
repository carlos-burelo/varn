//! The vocabulary of an instruction's layout: where an operand sits and what
//! it is.

use varn_core::OpCode;

/// Which byte of a code word.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Half {
    Hi,
    Lo,
}

/// One byte of an instruction. Word 0 is the opcode word, whose low byte is
/// the opcode itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Byte {
    pub word: usize,
    pub half: Half,
}

impl Byte {
    /// The byte's value in the instruction at `offset`; 0 past the end.
    pub fn read(self, code: &[u16], offset: usize) -> u8 {
        let w = code.get(offset + self.word).copied().unwrap_or(0);
        match self.half {
            Half::Hi => (w >> 8) as u8,
            Half::Lo => w as u8,
        }
    }

    pub fn write(self, code: &mut [u16], offset: usize, value: u8) {
        let w = &mut code[offset + self.word];
        *w = match self.half {
            Half::Hi => (*w & 0x00ff) | ((value as u16) << 8),
            Half::Lo => (*w & 0xff00) | value as u16,
        };
    }
}

/// A byte or a whole word of an instruction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum At {
    Byte(Byte),
    Word(usize),
}

impl At {
    pub fn read(self, code: &[u16], offset: usize) -> u16 {
        match self {
            At::Byte(b) => b.read(code, offset) as u16,
            At::Word(w) => code.get(offset + w).copied().unwrap_or(0),
        }
    }
}

/// How an instruction touches a register.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Read,
    Write,
    /// Read, then written: `ObjectMerge` folds into the object it reads.
    ReadWrite,
}

/// What a run of consecutive registers is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunKind {
    /// A call's arguments: the callee's frame is built from them, so they
    /// must stay contiguous.
    CallArgs,
    /// The elements of a collection being built.
    Values,
}

/// What a constant-pool index names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConstKind {
    /// Any literal, loaded as a value.
    Value,
    /// A string naming a property, method, global, class or variant.
    Name,
    Function,
    Shape,
    /// A module specifier.
    Module,
    /// A native op id (an integer).
    NativeOp,
    Symbol,
}

/// What an immediate field holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImmKind {
    /// A signed integer: `i16` in a word, `i8` in a byte.
    Int,
    /// The length of a list the layout already spells out.
    Count,
    Upvalue,
    /// An inline-cache slot.
    CallSite,
    /// A module global, relative to the module's base.
    GlobalSlot,
    /// A global of the native prelude, absolute.
    NativeGlobalSlot,
    ModuleSlot,
    /// A field's slot in its class's fixed layout, and its byte offset.
    FieldSlot,
    FieldOffset,
    /// A `RuntimeKind` a field is declared with.
    Tag,
    /// A `NumConv`.
    Conv,
    /// A `std:math` intrinsic's wire byte.
    Intrinsic,
    Flag,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operand {
    Reg {
        at: Byte,
        access: Access,
    },
    /// `count` registers from the one at `start`, read.
    Run {
        start: Byte,
        count: usize,
        kind: RunKind,
    },
    /// A register by convention rather than by operand (`this` is `r0`);
    /// never renumbered.
    Fixed {
        reg: u8,
        access: Access,
    },
    /// A constant-pool index, a whole word.
    Const {
        word: usize,
        kind: ConstKind,
    },
    Imm {
        at: At,
        kind: ImmKind,
    },
    /// A displacement in two words (high, low), measured from the end of
    /// the instruction.
    Jump {
        word: usize,
        backward: bool,
    },
}

/// An instruction's length and operands, in the order a listing shows them.
#[derive(Clone, Debug)]
pub struct Layout {
    pub op: OpCode,
    pub len: usize,
    pub operands: Vec<Operand>,
}

impl Layout {
    /// Control flow the register walkers cannot follow.
    pub fn opaque(&self) -> bool {
        matches!(self.op, OpCode::Jump | OpCode::Loop)
    }

    /// The runs of consecutive registers the instruction at `offset` reads:
    /// `(first, count, kind)`.
    pub fn runs<'a>(
        &'a self,
        code: &'a [u16],
        offset: usize,
    ) -> impl Iterator<Item = (u8, usize, RunKind)> + 'a {
        self.operands.iter().filter_map(move |o| match *o {
            Operand::Run { start, count, kind } => Some((start.read(code, offset), count, kind)),
            _ => None,
        })
    }

    /// The registers the instruction at `offset` reads through a register
    /// operand (not a run).
    pub fn read_registers<'a>(
        &'a self,
        code: &'a [u16],
        offset: usize,
    ) -> impl Iterator<Item = u8> + 'a {
        self.operands.iter().filter_map(move |o| match *o {
            Operand::Reg { at, access } if access != Access::Write => Some(at.read(code, offset)),
            _ => None,
        })
    }

    /// Where the instruction at `offset` jumps to, if it jumps.
    pub fn jump_target(&self, code: &[u16], offset: usize) -> Option<usize> {
        self.operands.iter().find_map(|o| match *o {
            Operand::Jump { word, backward } => {
                let hi = At::Word(word).read(code, offset) as usize;
                let lo = At::Word(word + 1).read(code, offset) as usize;
                let disp = (hi << 16) | lo;
                let end = offset + self.len;
                Some(if backward {
                    end.wrapping_sub(disp)
                } else {
                    end + disp
                })
            }
            _ => None,
        })
    }
}
