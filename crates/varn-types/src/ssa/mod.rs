use serde::{Deserialize, Serialize};

use crate::register_meta::{SlotClass, SlotKind};

mod op;
mod operands;
mod operators;

pub use op::SsaOp;
pub use operands::{SsaObjectSpreadPart, SsaSpread, SsaUpvalue, UPVALUE_LOCAL};
pub use operators::{DynBinOp, DynUnOp, SsaBinOp, SsaUnOp};

pub type SsaTy = SlotKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SsaValue {
    pub ty: SsaTy,
}

impl SsaValue {
    #[inline]
    pub fn class(&self) -> SlotClass {
        SlotClass::of_kind(self.ty)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SsaBlock {
    pub params: Vec<u32>,
    pub insts: Vec<SsaInst>,
    pub term: SsaTerm,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SsaInst {
    pub dest: Option<u32>,
    pub op: SsaOp,
    pub line: u32,
    pub ip: u32,
}

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

    pub captured: Vec<u32>,

    pub secondary_entries: Vec<SsaSecondaryEntry>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SsaEntryKind {
    Loop,
    Resume,
    Catch,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SsaSecondaryEntry {
    pub block: u32,
    pub ip: u32,

    pub live: Vec<u32>,

    pub kind: SsaEntryKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OsrMode {
    Resume,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OsrRequest {
    pub ip: usize,
    pub mode: OsrMode,
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

    pub fn secondary_at(&self, ip: usize) -> Option<&SsaSecondaryEntry> {
        self.secondary_entries
            .iter()
            .find(|e| e.ip as usize == ip && e.kind == SsaEntryKind::Resume)
            .or_else(|| {
                self.secondary_entries
                    .iter()
                    .find(|e| e.ip as usize == ip && e.kind == SsaEntryKind::Loop)
            })
    }

    pub fn catch_at(&self, ip: usize) -> Option<&SsaSecondaryEntry> {
        self.secondary_entries
            .iter()
            .find(|e| e.ip as usize == ip && e.kind == SsaEntryKind::Catch)
    }

    pub fn is_resume_entry(&self, ip: usize) -> bool {
        self.secondary_entries
            .iter()
            .any(|e| e.ip as usize == ip && e.kind == SsaEntryKind::Resume)
    }

    #[inline]
    pub fn captured_reg(&self, var: u32) -> Option<u32> {
        self.captured.get(var as usize).copied()
    }

    pub fn map_registers(&mut self, f: impl Fn(u32) -> u32) {
        for r in self.regs.iter_mut().chain(self.captured.iter_mut()) {
            *r = f(*r);
        }
    }
}

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

    pub fn unavailable(&self) -> Option<&str> {
        match self {
            Self::Available(_) => None,
            Self::Unavailable(why) => Some(why),
        }
    }
}
