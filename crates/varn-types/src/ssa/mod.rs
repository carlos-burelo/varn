




























use serde::{Deserialize, Serialize};

use crate::register_meta::{SlotClass, SlotKind};

mod op;
mod operators;

pub use op::{SsaObjectSpreadPart, SsaOp, SsaSpread, SsaUpvalue, UPVALUE_LOCAL};
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
    
    pub loop_headers: Vec<SsaLoopHeader>,
}







#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SsaLoopHeader {
    pub block: u32,
    pub ip: u32,
    
    pub live: Vec<u32>,
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

    
    pub fn loop_header_at(&self, ip: usize) -> Option<&SsaLoopHeader> {
        self.loop_headers.iter().find(|h| h.ip as usize == ip)
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
            loop_headers: Vec::new(),
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
