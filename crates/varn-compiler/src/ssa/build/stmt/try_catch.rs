//! `try` lowering in its three shapes, plus the finally blocks a jump out of
//! a guarded region has to run on its way.

use crate::hir::{HirCatch, HirStmt, HirType};
use crate::ssa::ir::{InstKind, Terminator};

use super::super::{Builder, Result, VarId};


impl Builder {
    pub(in crate::ssa::build) fn lower_try(
        &mut self,
        block: &[HirStmt],
        catch: &Option<HirCatch>,
        finally: &Option<Vec<HirStmt>>,
    ) -> Result<()> {
        match (catch, finally) {
            (None, Some(fin)) => self.lower_try_finally(block, fin),
            (Some(cc), None) => self.lower_try_catch(block, cc),
            (Some(cc), Some(fin)) => self.lower_try_catch_finally(block, cc, fin),
            (None, None) => self.lower_block(block),
        }
    }

    pub(in crate::ssa::build) fn lower_try_catch(&mut self, block: &[HirStmt], cc: &HirCatch) -> Result<()> {
        let try_entry = self.current;
        let catch_b = self.new_block();
        let exit_b = self.new_block();

        let try_val = self.emit(InstKind::Try { handler: catch_b }, HirType::Dynamic);

        self.lower_block(block)?;

        if self.is_open() {
            self.emit_effect(InstKind::PopTry);
            let from = self.current;
            self.set_term(Terminator::Jump {
                target: exit_b,
                args: Vec::new(),
            });
            self.add_pred(exit_b, from);
        }

        self.add_pred(catch_b, try_entry);
        self.seal_block(catch_b);

        self.current = catch_b;
        let caught_err = self.emit(InstKind::CatchParam { try_val }, HirType::Dynamic);
        if let Some(local) = cc.param {
            self.write_var(VarId::Local(local), catch_b, caught_err);
        }

        self.lower_block(&cc.body)?;

        if self.is_open() {
            let from = self.current;
            self.set_term(Terminator::Jump {
                target: exit_b,
                args: Vec::new(),
            });
            self.add_pred(exit_b, from);
        }

        self.seal_block(exit_b);
        self.current = exit_b;
        Ok(())
    }

    pub(in crate::ssa::build) fn lower_try_finally(&mut self, block: &[HirStmt], fin: &[HirStmt]) -> Result<()> {
        self.finally_stack.push(fin.to_vec());
        let try_entry = self.current;
        let handler_b = self.new_block();
        let exit_b = self.new_block();

        let try_val = self.emit(InstKind::Try { handler: handler_b }, HirType::Dynamic);

        self.lower_block(block)?;

        self.finally_stack.pop();

        if self.is_open() {
            self.emit_effect(InstKind::PopTry);
            self.lower_block(fin)?;
            let from = self.current;
            self.set_term(Terminator::Jump {
                target: exit_b,
                args: Vec::new(),
            });
            self.add_pred(exit_b, from);
        }

        self.add_pred(handler_b, try_entry);
        self.seal_block(handler_b);

        self.current = handler_b;
        let caught_err = self.emit(InstKind::CatchParam { try_val }, HirType::Dynamic);
        self.lower_block(fin)?;
        self.set_term(Terminator::Throw(caught_err));

        self.seal_block(exit_b);
        self.current = exit_b;
        Ok(())
    }

    pub(in crate::ssa::build) fn lower_try_catch_finally(
        &mut self,
        block: &[HirStmt],
        cc: &HirCatch,
        fin: &[HirStmt],
    ) -> Result<()> {
        self.finally_stack.push(fin.to_vec());
        let try_entry = self.current;
        let handler_b = self.new_block();
        let exit_b = self.new_block();

        let try_val = self.emit(InstKind::Try { handler: handler_b }, HirType::Dynamic);

        self.lower_block(block)?;

        self.finally_stack.pop();

        if self.is_open() {
            self.emit_effect(InstKind::PopTry);
            self.lower_block(fin)?;
            let from = self.current;
            self.set_term(Terminator::Jump {
                target: exit_b,
                args: Vec::new(),
            });
            self.add_pred(exit_b, from);
        }

        self.add_pred(handler_b, try_entry);
        self.seal_block(handler_b);

        self.current = handler_b;
        let caught_err = self.emit(InstKind::CatchParam { try_val }, HirType::Dynamic);

        self.finally_stack.push(fin.to_vec());
        let catch_entry = self.current;
        let handler2_b = self.new_block();
        let try_val2 = self.emit(
            InstKind::Try {
                handler: handler2_b,
            },
            HirType::Dynamic,
        );

        if let Some(local) = cc.param {
            self.write_var(VarId::Local(local), handler_b, caught_err);
        }

        self.lower_block(&cc.body)?;

        self.finally_stack.pop();

        if self.is_open() {
            self.emit_effect(InstKind::PopTry);
            self.lower_block(fin)?;
            let from = self.current;
            self.set_term(Terminator::Jump {
                target: exit_b,
                args: Vec::new(),
            });
            self.add_pred(exit_b, from);
        }

        self.add_pred(handler2_b, catch_entry);
        self.seal_block(handler2_b);

        self.current = handler2_b;
        let caught_err2 = self.emit(InstKind::CatchParam { try_val: try_val2 }, HirType::Dynamic);
        self.lower_block(fin)?;
        self.set_term(Terminator::Throw(caught_err2));

        self.seal_block(exit_b);
        self.current = exit_b;
        Ok(())
    }

    pub(in crate::ssa::build) fn emit_exiting_finallys(&mut self, depth: usize) -> Result<()> {
        if depth >= self.finally_stack.len() {
            return Ok(());
        }
        let fins: Vec<Vec<HirStmt>> = self.finally_stack[depth..].to_vec();
        for fin in fins.iter().rev() {
            self.emit_effect(InstKind::PopTry);
            self.lower_block(fin)?;
        }
        Ok(())
    }
}
