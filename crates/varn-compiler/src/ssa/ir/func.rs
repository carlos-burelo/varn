use super::control::Terminator;
use super::ids::{BlockId, Value};
use super::ops::InstKind;
use crate::hir::HirType;
use std::sync::Arc;

#[derive(Debug)]
pub struct SsaFunc {
    pub name: Arc<str>,

    pub entry: BlockId,
    pub blocks: Vec<Block>,

    pub values: Vec<super::ids::ValueDef>,

    pub is_async: bool,

    pub is_generator: bool,
}

impl SsaFunc {
    #[inline]
    pub fn block(&self, id: BlockId) -> &Block {
        &self.blocks[id.0 as usize]
    }

    #[inline]
    pub fn block_mut(&mut self, id: BlockId) -> &mut Block {
        &mut self.blocks[id.0 as usize]
    }

    #[inline]
    pub fn alloc_block(&mut self) -> BlockId {
        let id = BlockId(self.blocks.len() as u32);
        self.blocks.push(Block {
            params: Vec::new(),
            insts: Vec::new(),
            term: Terminator::Unreachable,
            term_line: 0,
            preds: Vec::new(),
        });
        id
    }

    pub fn value_ty(&self, v: Value) -> HirType {
        self.values[v.0 as usize].ty
    }

    pub fn replace_all_uses(&mut self, old: Value, new: Value) {
        let mut sub = |v: &mut Value| {
            if *v == old {
                *v = new;
            }
        };
        for block in &mut self.blocks {
            for inst in &mut block.insts {
                crate::ssa::uses::visit_uses_mut(&mut inst.kind, &mut sub);
            }
            crate::ssa::uses::visit_term_uses_mut(&mut block.term, &mut sub);
        }
    }
}

#[derive(Debug)]
pub struct Block {
    pub params: Vec<Value>,
    pub insts: Vec<Inst>,
    pub term: Terminator,
    pub term_line: u32,

    pub preds: Vec<BlockId>,
}

#[derive(Debug, Clone)]
pub struct Inst {
    pub dest: Option<Value>,
    pub kind: InstKind,
    pub line: u32,
}
