use varn_core::OpCode;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Half {
    Hi,
    Lo,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Byte {
    pub word: usize,
    pub half: Half,
}

impl Byte {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Read,
    Write,

    ReadWrite,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunKind {
    CallArgs,

    Values,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConstKind {
    Value,

    Name,
    Function,
    Shape,
    Layout,

    Module,

    NativeOp,
    Symbol,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImmKind {
    Int,

    Count,
    Upvalue,

    CallSite,

    GlobalSlot,

    NativeGlobalSlot,
    ModuleSlot,

    FieldSlot,
    FieldOffset,

    Tag,

    Conv,

    Intrinsic,
    Flag,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operand {
    Reg {
        at: Byte,
        access: Access,
    },

    Run {
        start: Byte,
        count: usize,
        kind: RunKind,
    },

    Fixed {
        reg: u8,
        access: Access,
    },

    Const {
        word: usize,
        kind: ConstKind,
    },
    Imm {
        at: At,
        kind: ImmKind,
    },

    Jump {
        word: usize,
        backward: bool,
    },
}

#[derive(Clone, Debug)]
pub struct Layout {
    pub op: OpCode,
    pub len: usize,
    pub operands: Vec<Operand>,
}

impl Layout {
    pub fn opaque(&self) -> bool {
        matches!(self.op, OpCode::Jump | OpCode::Loop)
    }

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
