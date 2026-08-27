//! `try` lowering in its shapes with multi-catch support, plus the finally blocks
//! a jump out of a guarded region has to run on its way.

use crate::hir::{HirBinOp, HirCatch, HirStmt, HirType, HirTypeTest, HirUnOp};
use crate::ssa::ir::{InstKind, Terminator, Value};

use super::super::{BlockId, Builder, Result, VarId};

impl Builder {
    pub(in crate::ssa::build) fn lower_try(
        &mut self,
        block: &[HirStmt],
        catches: &[HirCatch],
        finally: &Option<Vec<HirStmt>>,
    ) -> Result<()> {
        match (catches.is_empty(), finally) {
            (true, Some(fin)) => self.lower_try_finally(block, fin),
            (false, None) => self.lower_try_catches(block, catches),
            (false, Some(fin)) => self.lower_try_catches_finally(block, catches, fin),
            (true, None) => self.lower_block(block),
        }
    }

    pub(in crate::ssa::build) fn lower_try_catches(
        &mut self,
        block: &[HirStmt],
        catches: &[HirCatch],
    ) -> Result<()> {
        let try_entry = self.current;
        let landing_pad = self.new_block();
        let exit_b = self.new_block();

        let try_val = self.emit(InstKind::Try { handler: landing_pad }, HirType::Dynamic);

        self.open_try_regions.push(Vec::new());
        self.lower_block(block)?;
        self.open_try_regions.pop();

        if self.is_open() {
            self.emit_effect(InstKind::PopTry);
            let from = self.current;
            self.set_term(Terminator::Jump {
                target: exit_b,
                args: Vec::new(),
            });
            self.add_pred(exit_b, from);
        }

        self.add_pred(landing_pad, try_entry);
        self.seal_block(landing_pad);

        self.current = landing_pad;
        let caught_err = self.emit(InstKind::CatchParam { try_val }, HirType::Dynamic);

        self.lower_catches_dispatch(catches, caught_err, exit_b, None)?;

        self.seal_block(exit_b);
        self.current = exit_b;
        Ok(())
    }

    pub(in crate::ssa::build) fn lower_try_finally(
        &mut self,
        block: &[HirStmt],
        fin: &[HirStmt],
    ) -> Result<()> {
        self.open_try_regions.push(fin.to_vec());
        let try_entry = self.current;
        let handler_b = self.new_block();
        let exit_b = self.new_block();

        let try_val = self.emit(InstKind::Try { handler: handler_b }, HirType::Dynamic);

        self.lower_block(block)?;

        self.open_try_regions.pop();

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

    pub(in crate::ssa::build) fn lower_try_catches_finally(
        &mut self,
        block: &[HirStmt],
        catches: &[HirCatch],
        fin: &[HirStmt],
    ) -> Result<()> {
        self.open_try_regions.push(fin.to_vec());
        let try_entry = self.current;
        let handler_b = self.new_block();
        let exit_b = self.new_block();

        let try_val = self.emit(InstKind::Try { handler: handler_b }, HirType::Dynamic);

        self.lower_block(block)?;

        self.open_try_regions.pop();

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

        self.open_try_regions.push(fin.to_vec());
        let catch_entry = self.current;
        let handler2_b = self.new_block();
        let try_val2 = self.emit(
            InstKind::Try {
                handler: handler2_b,
            },
            HirType::Dynamic,
        );

        self.lower_catches_dispatch(catches, caught_err, exit_b, Some(fin))?;

        self.open_try_regions.pop();

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

    fn lower_catches_dispatch(
        &mut self,
        catches: &[HirCatch],
        caught_err: Value,
        exit_b: BlockId,
        fin: Option<&[HirStmt]>,
    ) -> Result<()> {
        let mut current_test_b = self.current;

        for cc in catches {
            self.current = current_test_b;

            if cc.type_tests.is_empty() {
                // Catch-all clause (unconditional match)
                if let Some(local) = cc.param {
                    self.write_var(VarId::Local(local), current_test_b, caught_err);
                }
                self.lower_block(&cc.body)?;
                if self.is_open() {
                    if let Some(fin_stmts) = fin {
                        self.emit_effect(InstKind::PopTry);
                        self.lower_block(fin_stmts)?;
                    }
                    let from = self.current;
                    self.set_term(Terminator::Jump {
                        target: exit_b,
                        args: Vec::new(),
                    });
                    self.add_pred(exit_b, from);
                }
                return Ok(());
            }

            let num_tests = cc.type_tests.len();
            let mut subtest_blocks = Vec::with_capacity(num_tests);
            subtest_blocks.push(current_test_b);
            for _ in 1..num_tests {
                subtest_blocks.push(self.new_block());
            }
            let match_body_b = self.new_block();
            let next_clause_test_b = self.new_block();

            for (k, test) in cc.type_tests.iter().enumerate() {
                self.current = subtest_blocks[k];
                if k > 0 {
                    self.seal_block(subtest_blocks[k]);
                }
                let cond = self.emit_single_type_test(test, caught_err);
                let from_test = self.current;
                let else_target = if k + 1 < num_tests {
                    subtest_blocks[k + 1]
                } else {
                    next_clause_test_b
                };
                self.set_term(Terminator::Branch {
                    cond,
                    then_blk: match_body_b,
                    then_args: Vec::new(),
                    else_blk: else_target,
                    else_args: Vec::new(),
                });
                self.add_pred(match_body_b, from_test);
                self.add_pred(else_target, from_test);
            }

            self.seal_block(match_body_b);
            self.seal_block(next_clause_test_b);

            // Match body
            self.current = match_body_b;
            if let Some(local) = cc.param {
                self.write_var(VarId::Local(local), match_body_b, caught_err);
            }
            self.lower_block(&cc.body)?;
            if self.is_open() {
                if let Some(fin_stmts) = fin {
                    self.emit_effect(InstKind::PopTry);
                    self.lower_block(fin_stmts)?;
                }
                let from = self.current;
                self.set_term(Terminator::Jump {
                    target: exit_b,
                    args: Vec::new(),
                });
                self.add_pred(exit_b, from);
            }

            current_test_b = next_clause_test_b;
        }

        // Unmatched exception fallthrough: rethrow caught_err
        self.current = current_test_b;
        if let Some(fin_stmts) = fin {
            self.emit_effect(InstKind::PopTry);
            self.lower_block(fin_stmts)?;
        }
        self.set_term(Terminator::Throw(caught_err));

        Ok(())
    }

    fn emit_single_type_test(&mut self, test: &HirTypeTest, operand: Value) -> Value {
        match test {
            HirTypeTest::IsNull => {
                self.emit(InstKind::IsNull { operand }, HirType::Bool)
            }
            HirTypeTest::IsArray => {
                self.emit(InstKind::IsArray { operand }, HirType::Bool)
            }
            HirTypeTest::TypeofEq(name) => {
                let t = self.emit(
                    InstKind::Unary {
                        op: HirUnOp::Typeof,
                        operand,
                        ty: HirType::Str,
                    },
                    HirType::Str,
                );
                let s = self.emit(InstKind::ConstStr(name.clone()), HirType::Str);
                self.emit(
                    InstKind::Binary {
                        op: HirBinOp::Eq,
                        lhs: t,
                        rhs: s,
                        ty: HirType::Dynamic,
                    },
                    HirType::Bool,
                )
            }
            HirTypeTest::Instanceof(name) => {
                let cls = self.emit(InstKind::LoadGlobal(name.clone()), HirType::Ref);
                self.emit(
                    InstKind::Binary {
                        op: HirBinOp::Instanceof,
                        lhs: operand,
                        rhs: cls,
                        ty: HirType::Dynamic,
                    },
                    HirType::Bool,
                )
            }
            HirTypeTest::AlwaysFalse => {
                self.emit(InstKind::ConstBool(false), HirType::Bool)
            }
        }
    }

    pub(in crate::ssa::build) fn emit_region_exits(&mut self, depth: usize) -> Result<()> {
        if depth >= self.open_try_regions.len() {
            return Ok(());
        }
        let fins: Vec<Vec<HirStmt>> = self.open_try_regions[depth..].to_vec();
        for fin in fins.iter().rev() {
            self.emit_effect(InstKind::PopTry);
            self.lower_block(fin)?;
        }
        Ok(())
    }
}
