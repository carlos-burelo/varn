use super::context::{Builder, Result};
use crate::hir::HirType;
use crate::ssa::ir::{Block, BlockId, Inst, InstKind, Terminator, Value, ValueDef, VarId};
use varn_tir::BackendTy;

impl<'m> Builder<'m> {
    pub(super) fn new_block(&mut self) -> BlockId {
        let id = BlockId(self.blocks.len() as u32);
        self.blocks.push(Block {
            params: Vec::new(),
            insts: Vec::new(),
            term: Terminator::Unreachable,
            term_line: 0,
            preds: Vec::new(),
        });
        self.sealed.push(false);
        self.terminated.push(false);
        id
    }

    pub(super) fn new_value(&mut self, ty: HirType) -> Value {
        let v = Value(self.values.len() as u32);
        self.values.push(ValueDef { ty });
        v
    }

    pub(super) fn value_ty(&self, v: Value) -> HirType {
        self.values[v.0 as usize].ty
    }

    pub(super) fn compact_field(
        &self,
        obj_ty: BackendTy,
        slot: u16,
    ) -> Option<(u32, Option<varn_core::RuntimeKind>)> {
        let BackendTy::Class(c) = obj_ty.non_nullable(&self.tir.types) else {
            return None;
        };
        let f = self
            .tir
            .class(c)?
            .layout
            .get_field_by_index(slot as usize)?;
        Some((f.offset, f.kind))
    }

    pub(super) fn block_mut(&mut self, id: BlockId) -> &mut Block {
        &mut self.blocks[id.0 as usize]
    }

    pub(super) fn is_open(&self) -> bool {
        !self.terminated[self.current.0 as usize]
    }

    pub(super) fn set_term(&mut self, term: Terminator) {
        self.terminated[self.current.0 as usize] = true;
        let line = self.cur_line;
        let blk = self.block_mut(self.current);
        blk.term = term;
        blk.term_line = line;
    }

    pub(super) fn add_pred(&mut self, block: BlockId, pred: BlockId) {
        self.block_mut(block).preds.push(pred);
    }

    pub(super) fn emit(&mut self, kind: InstKind, ty: HirType) -> Value {
        let dest = self.new_value(ty);
        let line = self.cur_line;
        self.block_mut(self.current).insts.push(Inst {
            dest: Some(dest),
            kind,
            line,
        });
        dest
    }

    pub(super) fn emit_effect(&mut self, kind: InstKind) {
        let line = self.cur_line;
        self.block_mut(self.current).insts.push(Inst {
            dest: None,
            kind,
            line,
        });
    }

    pub(super) fn write_var(&mut self, var: VarId, block: BlockId, value: Value) {
        self.var_ty.insert(var, self.values[value.0 as usize].ty);
        self.defs.insert((var, block), value);
    }

    pub(super) fn read_var(&mut self, var: VarId, block: BlockId) -> Result<Value> {
        if let Some(v) = self.defs.get(&(var, block)) {
            return Ok(*v);
        }
        self.read_var_recursive(var, block)
    }

    pub(super) fn read_var_recursive(&mut self, var: VarId, block: BlockId) -> Result<Value> {
        let ty = *self.var_ty.get(&var).ok_or(crate::OptError::Unsupported(
            "from_tir: read of undefined variable",
        ))?;
        if !self.sealed[block.0 as usize] {
            let phi = self.add_block_param(block, ty);
            self.incomplete_phis
                .entry(block)
                .or_default()
                .push((var, phi));
            self.write_var(var, block, phi);
            return Ok(phi);
        }
        let preds = self.blocks[block.0 as usize].preds.clone();
        let val = if preds.len() == 1 {
            self.read_var(var, preds[0])?
        } else {
            let phi = self.add_block_param(block, ty);
            self.write_var(var, block, phi);
            self.add_phi_operands(var, block, phi)?;
            phi
        };
        self.write_var(var, block, val);
        Ok(val)
    }

    pub(super) fn add_block_param(&mut self, block: BlockId, ty: HirType) -> Value {
        let v = self.new_value(ty);
        self.block_mut(block).params.push(v);
        v
    }

    pub(super) fn add_phi_operands(
        &mut self,
        var: VarId,
        block: BlockId,
        phi: Value,
    ) -> Result<()> {
        let pos = self.blocks[block.0 as usize]
            .params
            .iter()
            .position(|p| *p == phi)
            .expect("phi is a param of block");
        for pred in self.blocks[block.0 as usize].preds.clone() {
            let arg = self.read_var(var, pred)?;
            self.append_edge_arg(pred, block, pos, arg);
        }
        Ok(())
    }

    pub(super) fn append_edge_arg(
        &mut self,
        pred: BlockId,
        block: BlockId,
        pos: usize,
        arg: Value,
    ) {
        match &mut self.block_mut(pred).term {
            Terminator::Jump { target, args } if *target == block => {
                debug_assert_eq!(args.len(), pos);
                args.push(arg);
            }
            Terminator::Branch {
                then_blk,
                then_args,
                else_blk,
                else_args,
                ..
            } => {
                if *then_blk == block {
                    then_args.push(arg);
                }
                if *else_blk == block {
                    else_args.push(arg);
                }
            }
            _ => panic!("predecessor {pred:?} has no edge to {block:?}"),
        }
    }

    pub(super) fn seal_block(&mut self, block: BlockId) {
        if let Some(phis) = self.incomplete_phis.remove(&block) {
            for (var, phi) in phis {
                let _ = self.add_phi_operands(var, block, phi);
            }
        }
        self.sealed[block.0 as usize] = true;
    }
}
