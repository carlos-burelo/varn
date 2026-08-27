//! Loop lowering: `while`, `do/while`, the classic three-clause `for`, and the
//! two iteration forms (`for..of`, `for..in`).

use std::rc::Rc;

use crate::hir::{HirBinOp, HirExpr, HirStmt, HirType, LocalId};
use crate::ssa::ir::{InstKind, Terminator};

use super::super::{Builder, LoopCtx, Result, VarId};

impl Builder {
    pub(in crate::ssa::build) fn lower_while(
        &mut self,
        test: &HirExpr,
        body: &[HirStmt],
    ) -> Result<()> {
        let pre = self.current;
        let header = self.new_block();
        self.set_term(Terminator::Jump {
            target: header,
            args: Vec::new(),
        });
        self.add_pred(header, pre);

        self.current = header;
        let cond = self.lower_expr(test)?;
        let test_end = self.current;
        let body_b = self.new_block();
        let exit_b = self.new_block();
        self.set_term(Terminator::Branch {
            cond,
            then_blk: body_b,
            then_args: Vec::new(),
            else_blk: exit_b,
            else_args: Vec::new(),
        });
        self.add_pred(body_b, test_end);
        self.add_pred(exit_b, test_end);
        self.seal_block(body_b);

        self.loops.push(LoopCtx {
            continue_target: header,
            break_target: exit_b,
            try_region_depth: self.open_try_regions.len(),
        });
        self.current = body_b;
        self.lower_block(body)?;
        if self.is_open() {
            let from = self.current;
            self.set_term(Terminator::Jump {
                target: header,
                args: Vec::new(),
            });
            self.add_pred(header, from);
        }
        self.loops.pop();

        self.seal_block(header);
        self.seal_block(exit_b);
        self.current = exit_b;
        Ok(())
    }

    pub(in crate::ssa::build) fn lower_for_classic(
        &mut self,
        test: &HirExpr,
        update: &[HirStmt],
        body: &[HirStmt],
    ) -> Result<()> {
        let pre = self.current;
        let header = self.new_block();
        self.set_term(Terminator::Jump {
            target: header,
            args: Vec::new(),
        });
        self.add_pred(header, pre);

        self.current = header;
        let cond = self.lower_expr(test)?;
        let test_end = self.current;
        let body_b = self.new_block();
        let update_b = self.new_block();
        let exit_b = self.new_block();
        self.set_term(Terminator::Branch {
            cond,
            then_blk: body_b,
            then_args: Vec::new(),
            else_blk: exit_b,
            else_args: Vec::new(),
        });
        self.add_pred(body_b, test_end);
        self.add_pred(exit_b, test_end);
        self.seal_block(body_b);

        self.loops.push(LoopCtx {
            continue_target: update_b,
            break_target: exit_b,
            try_region_depth: self.open_try_regions.len(),
        });
        self.current = body_b;
        self.lower_block(body)?;
        if self.is_open() {
            let from = self.current;
            self.set_term(Terminator::Jump {
                target: update_b,
                args: Vec::new(),
            });
            self.add_pred(update_b, from);
        }
        self.loops.pop();

        self.seal_block(update_b);
        self.current = update_b;
        self.lower_block(update)?;
        if self.is_open() {
            let from = self.current;
            self.set_term(Terminator::Jump {
                target: header,
                args: Vec::new(),
            });
            self.add_pred(header, from);
        }

        self.seal_block(header);
        self.seal_block(exit_b);
        self.current = exit_b;
        Ok(())
    }

    pub(in crate::ssa::build) fn lower_for_of(
        &mut self,
        var: LocalId,
        iterable: &HirExpr,
        body: &[HirStmt],
        is_await: bool,
    ) -> Result<()> {
        let it = self.lower_expr(iterable)?;
        let iter_fn = self.emit(
            InstKind::GetSymbol {
                object: it,
                is_async: is_await,
            },
            HirType::Ref,
        );
        let iterator = self.emit(
            InstKind::IterCall {
                callee: iter_fn,
                recv: it,
            },
            HirType::Ref,
        );

        let pre = self.current;
        let header = self.new_block();
        self.set_term(Terminator::Jump {
            target: header,
            args: Vec::new(),
        });
        self.add_pred(header, pre);

        self.current = header;
        let next_fn = self.emit(
            InstKind::GetProperty {
                object: iterator,
                name: Rc::from("next"),
            },
            HirType::Ref,
        );
        let mut result = self.emit(
            InstKind::IterCall {
                callee: next_fn,
                recv: iterator,
            },
            HirType::Ref,
        );
        if is_await {
            result = self.emit(InstKind::Await { operand: result }, HirType::Dynamic);
        }
        let done = self.emit(
            InstKind::GetProperty {
                object: result,
                name: Rc::from("done"),
            },
            HirType::Bool,
        );
        let body_b = self.new_block();
        let exit_b = self.new_block();

        self.set_term(Terminator::Branch {
            cond: done,
            then_blk: exit_b,
            then_args: Vec::new(),
            else_blk: body_b,
            else_args: Vec::new(),
        });
        self.add_pred(exit_b, header);
        self.add_pred(body_b, header);
        self.seal_block(body_b);

        self.loops.push(LoopCtx {
            continue_target: header,
            break_target: exit_b,
            try_region_depth: self.open_try_regions.len(),
        });
        self.current = body_b;
        let value = self.emit(
            InstKind::GetProperty {
                object: result,
                name: Rc::from("value"),
            },
            HirType::Dynamic,
        );
        self.write_var(VarId::Local(var), body_b, value);
        self.lower_block(body)?;
        if self.is_open() {
            let from = self.current;
            self.set_term(Terminator::Jump {
                target: header,
                args: Vec::new(),
            });
            self.add_pred(header, from);
        }
        self.loops.pop();

        self.seal_block(header);
        self.seal_block(exit_b);
        self.current = exit_b;
        Ok(())
    }

    pub(in crate::ssa::build) fn lower_for_in(
        &mut self,
        var: LocalId,
        object: &HirExpr,
        body: &[HirStmt],
    ) -> Result<()> {
        let obj = self.lower_expr(object)?;
        let keys = self.emit(InstKind::ObjectKeys { operand: obj }, HirType::Ref);
        let idx_var = self.fresh_synthetic();
        let zero = self.emit(InstKind::ConstInt(0), HirType::Int);
        self.write_var(idx_var, self.current, zero);

        let pre = self.current;
        let header = self.new_block();
        self.set_term(Terminator::Jump {
            target: header,
            args: Vec::new(),
        });
        self.add_pred(header, pre);

        self.current = header;
        let len = self.emit(
            InstKind::GetProperty {
                object: keys,
                name: Rc::from(varn_core::MemberKey::Length.as_str()),
            },
            HirType::Int,
        );
        let idx = self.read_var(idx_var, header)?;
        let cond = self.emit(
            InstKind::Binary {
                op: HirBinOp::Lt,
                lhs: idx,
                rhs: len,
                ty: HirType::Bool,
            },
            HirType::Bool,
        );
        let body_b = self.new_block();
        let update_b = self.new_block();
        let exit_b = self.new_block();
        self.set_term(Terminator::Branch {
            cond,
            then_blk: body_b,
            then_args: Vec::new(),
            else_blk: exit_b,
            else_args: Vec::new(),
        });
        self.add_pred(body_b, header);
        self.add_pred(exit_b, header);
        self.seal_block(body_b);

        self.loops.push(LoopCtx {
            continue_target: update_b,
            break_target: exit_b,
            try_region_depth: self.open_try_regions.len(),
        });
        self.current = body_b;
        let idx_b = self.read_var(idx_var, body_b)?;
        let elem = self.emit(
            InstKind::GetIndex {
                object: keys,
                index: idx_b,
            },
            HirType::Dynamic,
        );
        self.write_var(VarId::Local(var), body_b, elem);
        self.lower_block(body)?;
        if self.is_open() {
            let from = self.current;
            self.set_term(Terminator::Jump {
                target: update_b,
                args: Vec::new(),
            });
            self.add_pred(update_b, from);
        }
        self.loops.pop();

        self.seal_block(update_b);
        self.current = update_b;
        let idx_u = self.read_var(idx_var, update_b)?;
        let one = self.emit(InstKind::ConstInt(1), HirType::Int);
        let next = self.emit(
            InstKind::Binary {
                op: HirBinOp::Add,
                lhs: idx_u,
                rhs: one,
                ty: HirType::Int,
            },
            HirType::Int,
        );
        self.write_var(idx_var, update_b, next);
        if self.is_open() {
            let from = self.current;
            self.set_term(Terminator::Jump {
                target: header,
                args: Vec::new(),
            });
            self.add_pred(header, from);
        }

        self.seal_block(header);
        self.seal_block(exit_b);
        self.current = exit_b;
        Ok(())
    }

    pub(in crate::ssa::build) fn lower_do_while(
        &mut self,
        body: &[HirStmt],
        test: &HirExpr,
    ) -> Result<()> {
        let pre = self.current;
        let body_b = self.new_block();
        self.set_term(Terminator::Jump {
            target: body_b,
            args: Vec::new(),
        });
        self.add_pred(body_b, pre);

        let latch = self.new_block();
        let exit_b = self.new_block();

        self.loops.push(LoopCtx {
            continue_target: latch,
            break_target: exit_b,
            try_region_depth: self.open_try_regions.len(),
        });
        self.current = body_b;
        self.lower_block(body)?;
        if self.is_open() {
            let from = self.current;
            self.set_term(Terminator::Jump {
                target: latch,
                args: Vec::new(),
            });
            self.add_pred(latch, from);
        }
        self.loops.pop();

        self.seal_block(latch);
        self.current = latch;
        let cond = self.lower_expr(test)?;
        let latch_end = self.current;
        self.set_term(Terminator::Branch {
            cond,
            then_blk: body_b,
            then_args: Vec::new(),
            else_blk: exit_b,
            else_args: Vec::new(),
        });
        self.add_pred(body_b, latch_end);
        self.add_pred(exit_b, latch_end);
        self.seal_block(body_b);
        self.seal_block(exit_b);
        self.current = exit_b;
        Ok(())
    }
}
