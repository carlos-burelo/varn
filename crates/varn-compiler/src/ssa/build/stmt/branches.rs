//! Branching control flow: `if`, the value-producing branch, and `switch`.

use crate::hir::{HirBinOp, HirExpr, HirStmt, HirSwitchCase, HirType};
use crate::ssa::ir::{BlockId, InstKind, Terminator, Value};

use super::super::{Builder, LoopCtx, Result};

impl Builder {
    pub(in crate::ssa::build) fn lower_if(
        &mut self,
        test: &HirExpr,
        then_body: &[HirStmt],
        else_body: &[HirStmt],
    ) -> Result<()> {
        let cond = self.lower_expr(test)?;
        let entry = self.current;
        let then_b = self.new_block();
        let merge = self.new_block();
        let else_b = if else_body.is_empty() {
            None
        } else {
            Some(self.new_block())
        };
        let else_target = else_b.unwrap_or(merge);

        self.set_term(Terminator::Branch {
            cond,
            then_blk: then_b,
            then_args: Vec::new(),
            else_blk: else_target,
            else_args: Vec::new(),
        });
        self.add_pred(then_b, entry);
        self.add_pred(else_target, entry);

        self.seal_block(then_b);
        if let Some(eb) = else_b {
            self.seal_block(eb);
        }

        self.current = then_b;
        self.lower_block(then_body)?;

        if self.is_open() {
            let from = self.current;
            self.set_term(Terminator::Jump {
                target: merge,
                args: Vec::new(),
            });
            self.add_pred(merge, from);
        }

        if let Some(eb) = else_b {
            self.current = eb;
            self.lower_block(else_body)?;
            if self.is_open() {
                let from = self.current;
                self.set_term(Terminator::Jump {
                    target: merge,
                    args: Vec::new(),
                });
                self.add_pred(merge, from);
            }
        }

        self.seal_block(merge);
        self.current = merge;
        Ok(())
    }

    pub(in crate::ssa::build) fn lower_branch_value(
        &mut self,
        cond: Value,
        then_fn: impl FnOnce(&mut Self) -> Result<Value>,
        else_fn: impl FnOnce(&mut Self) -> Result<Value>,
        ty: HirType,
    ) -> Result<Value> {
        let entry = self.current;
        let then_b = self.new_block();
        let else_b = self.new_block();
        let merge = self.new_block();
        self.set_term(Terminator::Branch {
            cond,
            then_blk: then_b,
            then_args: Vec::new(),
            else_blk: else_b,
            else_args: Vec::new(),
        });
        self.add_pred(then_b, entry);
        self.add_pred(else_b, entry);
        self.seal_block(then_b);
        self.seal_block(else_b);

        self.current = then_b;
        let vt = then_fn(self)?;
        let t_end = self.current;
        self.set_term(Terminator::Jump {
            target: merge,
            args: vec![vt],
        });
        self.add_pred(merge, t_end);

        self.current = else_b;
        let ve = else_fn(self)?;
        let e_end = self.current;
        self.set_term(Terminator::Jump {
            target: merge,
            args: vec![ve],
        });
        self.add_pred(merge, e_end);

        self.seal_block(merge);
        self.current = merge;

        Ok(self.add_block_param(merge, ty))
    }

    pub(in crate::ssa::build) fn lower_switch(
        &mut self,
        disc: &HirExpr,
        cases: &[HirSwitchCase],
    ) -> Result<()> {
        let dv = self.lower_expr(disc)?;
        let n = cases.len();
        let bodies: Vec<BlockId> = (0..n).map(|_| self.new_block()).collect();
        let exit = self.new_block();
        let default_idx = cases.iter().position(|c| c.test.is_none());

        let mut cur = self.current;
        for (i, case) in cases.iter().enumerate() {
            if let Some(test) = &case.test {
                self.current = cur;
                let val = self.lower_expr(test)?;
                let eq = self.emit(
                    InstKind::Binary {
                        op: HirBinOp::Eq,
                        lhs: dv,
                        rhs: val,
                        ty: HirType::Dynamic,
                    },
                    HirType::Bool,
                );
                let next = self.new_block();
                self.set_term(Terminator::Branch {
                    cond: eq,
                    then_blk: bodies[i],
                    then_args: Vec::new(),
                    else_blk: next,
                    else_args: Vec::new(),
                });
                self.add_pred(bodies[i], cur);
                self.add_pred(next, cur);
                self.seal_block(next);
                cur = next;
            }
        }

        let no_match = match default_idx {
            Some(d) => bodies[d],
            None => exit,
        };
        self.current = cur;
        self.set_term(Terminator::Jump {
            target: no_match,
            args: Vec::new(),
        });
        self.add_pred(no_match, cur);

        let cont = self.loops.last().map(|c| c.continue_target).unwrap_or(exit);
        self.loops.push(LoopCtx {
            continue_target: cont,
            break_target: exit,
            try_region_depth: self.open_try_regions.len(),
        });
        for (i, case) in cases.iter().enumerate() {
            self.seal_block(bodies[i]);
            self.current = bodies[i];
            self.lower_block(&case.body)?;
            if self.is_open() {
                let from = self.current;
                let target = if i + 1 < n { bodies[i + 1] } else { exit };
                self.set_term(Terminator::Jump {
                    target,
                    args: Vec::new(),
                });
                self.add_pred(target, from);
            }
        }
        self.loops.pop();

        self.seal_block(exit);
        self.current = exit;
        Ok(())
    }
}
