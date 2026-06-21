//! HIR -> SSA: statement and control-flow lowering.

use std::rc::Rc;

use crate::hir::{HirBinOp, HirExpr, HirStmt, HirSwitchCase, HirType, LocalId};
use crate::ssa::ir::{BlockId, InstKind, Terminator, Value};
use crate::OptError;

use super::{binding_var, Builder, LoopCtx, Result, VarId};

impl Builder {
    pub(super) fn lower_block(&mut self, stmts: &[HirStmt]) -> Result<()> {
        for stmt in stmts {
            if !self.is_open() {
                break; // remaining statements are unreachable
            }
            self.lower_stmt(stmt)?;
        }
        Ok(())
    }

    pub(super) fn lower_stmt(&mut self, stmt: &HirStmt) -> Result<()> {
        match stmt {
            HirStmt::Expr(e) => {
                self.lower_expr(e)?;
                Ok(())
            }
            HirStmt::Let { local, value, .. } => {
                let v = self.lower_expr(value)?;
                self.write_var(VarId::Local(*local), self.current, v);
                Ok(())
            }
            HirStmt::Assign { target, value } => {
                let var = binding_var(target)?;
                let v = self.lower_expr(value)?;
                self.write_var(var, self.current, v);
                Ok(())
            }
            HirStmt::SetMember { object, name, value } => {
                let o = self.lower_expr(object)?;
                let v = self.lower_expr(value)?;
                self.emit_effect(InstKind::SetProperty { object: o, name: name.clone(), value: v });
                Ok(())
            }
            HirStmt::SetIndex { object, index, value } => {
                let o = self.lower_expr(object)?;
                let i = self.lower_expr(index)?;
                let v = self.lower_expr(value)?;
                self.emit_effect(InstKind::SetIndex { object: o, index: i, value: v });
                Ok(())
            }
            HirStmt::Return(value) => {
                let v = match value {
                    Some(e) => Some(self.lower_expr(e)?),
                    None => None,
                };
                self.set_term(Terminator::Return(v));
                Ok(())
            }
            HirStmt::Throw(e) => {
                let v = self.lower_expr(e)?;
                self.set_term(Terminator::Throw(v));
                Ok(())
            }
            HirStmt::If {
                test,
                then_body,
                else_body,
            } => self.lower_if(test, then_body, else_body),
            HirStmt::While { test, body } => self.lower_while(test, body),
            HirStmt::ForClassic {
                test,
                update,
                body,
            } => self.lower_for_classic(test, update, body),
            HirStmt::DoWhile { body, test } => self.lower_do_while(body, test),
            HirStmt::ForIn { var, object, body } => self.lower_for_in(*var, object, body),
            HirStmt::ForOf { var, iterable, body, is_await } => {
                if *is_await {
                    return Err(OptError::Unsupported("ssa: for-await-of"));
                }
                self.lower_for_of(*var, iterable, body)
            }
            HirStmt::Switch { disc, cases } => self.lower_switch(disc, cases),
            HirStmt::Break => self.lower_jump_out(|c| c.break_target, "ssa: break outside loop"),
            HirStmt::Continue => {
                self.lower_jump_out(|c| c.continue_target, "ssa: continue outside loop")
            }
            _ => Err(OptError::Unsupported("ssa: statement kind")),
        }
    }

    /// Lower `break`/`continue`: terminate the current block with a jump to the
    /// innermost loop's chosen target and record the edge. Subsequent statements
    /// in the block are unreachable (`lower_block` stops at the closed block).
    pub(super) fn lower_jump_out(
        &mut self,
        pick: impl Fn(&LoopCtx) -> BlockId,
        err: &'static str,
    ) -> Result<()> {
        let target = pick(self.loops.last().ok_or(OptError::Unsupported(err))?);
        let from = self.current;
        self.set_term(Terminator::Jump {
            target,
            args: Vec::new(),
        });
        self.add_pred(target, from);
        Ok(())
    }

    pub(super) fn lower_if(
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
        // then/else predecessors are fully known now; seal before lowering so
        // their single-predecessor reads don't spawn spurious phis.
        self.seal_block(then_b);
        if let Some(eb) = else_b {
            self.seal_block(eb);
        }

        self.current = then_b;
        self.lower_block(then_body)?;
        // The block that actually falls through to `merge` is whatever block is
        // current after the body — not `then_b`, which differs once the body
        // contains nested control flow.
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

    /// Branch on `cond` (truthy), evaluate one arm in each successor, and merge
    /// their values through a single block parameter (the result phi). Used for
    /// short-circuiting logicals and the ternary. The arm closures must not
    /// terminate the block (pure expressions; any unsupported sub-expr makes the
    /// whole function fall back before reaching here).
    pub(super) fn lower_branch_value(
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
        self.set_term(Terminator::Jump { target: merge, args: vec![vt] });
        self.add_pred(merge, t_end);

        self.current = else_b;
        let ve = else_fn(self)?;
        let e_end = self.current;
        self.set_term(Terminator::Jump { target: merge, args: vec![ve] });
        self.add_pred(merge, e_end);

        self.seal_block(merge);
        self.current = merge;
        // The result phi: a single block param fed by the two edge args above.
        Ok(self.add_block_param(merge, ty))
    }

    /// `while (test) body` — a pre-tested loop. The header is left unsealed while
    /// the body is lowered so reads of variables modified in the loop spawn
    /// incomplete phis (Braun); the back-edge supplies their loop-carried
    /// operand and `seal_block(header)` fills them.
    pub(super) fn lower_while(&mut self, test: &HirExpr, body: &[HirStmt]) -> Result<()> {
        let pre = self.current;
        let header = self.new_block();
        self.set_term(Terminator::Jump {
            target: header,
            args: Vec::new(),
        });
        self.add_pred(header, pre);
        // header intentionally NOT sealed: the back-edge is built below.

        self.current = header;
        let cond = self.lower_expr(test)?;
        let body_b = self.new_block();
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
        self.seal_block(body_b); // sole predecessor (header) is known

        self.loops.push(LoopCtx {
            continue_target: header,
            break_target: exit_b,
        });
        self.current = body_b;
        self.lower_block(body)?;
        if self.is_open() {
            let from = self.current;
            self.set_term(Terminator::Jump {
                target: header,
                args: Vec::new(),
            });
            self.add_pred(header, from); // back-edge
        }
        self.loops.pop();

        self.seal_block(header); // back-edge + any `continue` edges now present
        self.seal_block(exit_b); // header's else + any `break` edges now present
        self.current = exit_b;
        Ok(())
    }

    /// C-style `for`: `test` at the header, `update` after the body in its own
    /// block (where `continue` lands, so the increment is never skipped), then
    /// the back-edge to the header. `init` is lowered as ordinary statements
    /// before this node.
    pub(super) fn lower_for_classic(
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

        self.seal_block(update_b); // body fall-through + any `continue` edges
        self.current = update_b;
        self.lower_block(update)?;
        if self.is_open() {
            let from = self.current;
            self.set_term(Terminator::Jump {
                target: header,
                args: Vec::new(),
            });
            self.add_pred(header, from); // back-edge
        }

        self.seal_block(header);
        self.seal_block(exit_b);
        self.current = exit_b;
        Ok(())
    }

    /// `switch (disc) { cases }` — a test chain (`disc == val` → branch to the
    /// case body, else the next test) then the bodies in source order, **falling
    /// through** to each other. `default` is reached by the no-match jump. A
    /// `break`-only scope (exit); `continue` targets the enclosing loop. All edges
    /// are forward (no back-edges), so every block is sealed once its predecessors
    /// — its test branch, the no-match jump (default), and the previous body's
    /// fall-through — are wired, which all happens before it is lowered.
    pub(super) fn lower_switch(&mut self, disc: &HirExpr, cases: &[HirSwitchCase]) -> Result<()> {
        let dv = self.lower_expr(disc)?;
        let n = cases.len();
        let bodies: Vec<BlockId> = (0..n).map(|_| self.new_block()).collect();
        let exit = self.new_block();
        let default_idx = cases.iter().position(|c| c.test.is_none());

        // Test chain, starting in the current block.
        let mut cur = self.current;
        for (i, case) in cases.iter().enumerate() {
            if let Some(test) = &case.test {
                self.current = cur;
                let val = self.lower_expr(test)?;
                let eq = self.emit(
                    InstKind::Binary { op: HirBinOp::Eq, lhs: dv, rhs: val, ty: HirType::Dynamic },
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
        // No test matched → the default body, or past the switch.
        let no_match = match default_idx {
            Some(d) => bodies[d],
            None => exit,
        };
        self.current = cur;
        self.set_term(Terminator::Jump { target: no_match, args: Vec::new() });
        self.add_pred(no_match, cur);

        // Bodies, falling through. `break` → exit; `continue` → enclosing loop.
        let cont = self.loops.last().map(|c| c.continue_target).unwrap_or(exit);
        self.loops.push(LoopCtx { continue_target: cont, break_target: exit });
        for (i, case) in cases.iter().enumerate() {
            self.seal_block(bodies[i]);
            self.current = bodies[i];
            self.lower_block(&case.body)?;
            if self.is_open() {
                let from = self.current;
                let target = if i + 1 < n { bodies[i + 1] } else { exit };
                self.set_term(Terminator::Jump { target, args: Vec::new() });
                self.add_pred(target, from);
            }
        }
        self.loops.pop();

        self.seal_block(exit);
        self.current = exit;
        Ok(())
    }

    /// `for (var of iterable) body` — the iterator protocol: `iter =
    /// iterable[Symbol.iterator]()`, then each iteration `r = iter.next()`, exit
    /// when `r.done`, bind `r.value` to `var`. The iterator is loop-invariant
    /// (dominates the header); body-modified vars become loop phis via Braun (the
    /// header is unsealed during the body). `continue` re-runs the header.
    pub(super) fn lower_for_of(&mut self, var: LocalId, iterable: &HirExpr, body: &[HirStmt]) -> Result<()> {
        let it = self.lower_expr(iterable)?;
        let iter_fn = self.emit(InstKind::GetSymbol { object: it, is_async: false }, HirType::Ref);
        let iterator = self.emit(InstKind::IterCall { callee: iter_fn, recv: it }, HirType::Ref);

        let pre = self.current;
        let header = self.new_block();
        self.set_term(Terminator::Jump { target: header, args: Vec::new() });
        self.add_pred(header, pre);

        self.current = header;
        let next_fn = self.emit(
            InstKind::GetProperty { object: iterator, name: Rc::from("next") },
            HirType::Ref,
        );
        let result = self.emit(InstKind::IterCall { callee: next_fn, recv: iterator }, HirType::Ref);
        let done = self.emit(
            InstKind::GetProperty { object: result, name: Rc::from("done") },
            HirType::Bool,
        );
        let body_b = self.new_block();
        let exit_b = self.new_block();
        // `done` truthy → exit; else → body.
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
        });
        self.current = body_b;
        let value = self.emit(
            InstKind::GetProperty { object: result, name: Rc::from("value") },
            HirType::Dynamic,
        );
        self.write_var(VarId::Local(var), body_b, value);
        self.lower_block(body)?;
        if self.is_open() {
            let from = self.current;
            self.set_term(Terminator::Jump { target: header, args: Vec::new() });
            self.add_pred(header, from);
        }
        self.loops.pop();

        self.seal_block(header);
        self.seal_block(exit_b);
        self.current = exit_b;
        Ok(())
    }

    /// `for (var in object) body` — `ObjectKeys` + an index loop. The index is a
    /// synthetic loop variable carried by an ordinary Braun phi; `var` is bound to
    /// `keys[idx]` each iteration. `continue` lands on the index increment.
    pub(super) fn lower_for_in(&mut self, var: LocalId, object: &HirExpr, body: &[HirStmt]) -> Result<()> {
        let obj = self.lower_expr(object)?;
        let keys = self.emit(InstKind::ObjectKeys { operand: obj }, HirType::Ref);
        let idx_var = self.fresh_synthetic();
        let zero = self.emit(InstKind::ConstInt(0), HirType::Int);
        self.write_var(idx_var, self.current, zero);

        let pre = self.current;
        let header = self.new_block();
        self.set_term(Terminator::Jump { target: header, args: Vec::new() });
        self.add_pred(header, pre);

        self.current = header;
        let len = self.emit(
            InstKind::GetProperty { object: keys, name: Rc::from("length") },
            HirType::Int,
        );
        let idx = self.read_var(idx_var, header)?;
        let cond = self.emit(
            InstKind::Binary { op: HirBinOp::Lt, lhs: idx, rhs: len, ty: HirType::Bool },
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
        });
        self.current = body_b;
        let idx_b = self.read_var(idx_var, body_b)?;
        let elem = self.emit(
            InstKind::GetIndex { object: keys, index: idx_b },
            HirType::Dynamic,
        );
        self.write_var(VarId::Local(var), body_b, elem);
        self.lower_block(body)?;
        if self.is_open() {
            let from = self.current;
            self.set_term(Terminator::Jump { target: update_b, args: Vec::new() });
            self.add_pred(update_b, from);
        }
        self.loops.pop();

        self.seal_block(update_b);
        self.current = update_b;
        let idx_u = self.read_var(idx_var, update_b)?;
        let one = self.emit(InstKind::ConstInt(1), HirType::Int);
        let next = self.emit(
            InstKind::Binary { op: HirBinOp::Add, lhs: idx_u, rhs: one, ty: HirType::Int },
            HirType::Int,
        );
        self.write_var(idx_var, update_b, next);
        if self.is_open() {
            let from = self.current;
            self.set_term(Terminator::Jump { target: header, args: Vec::new() });
            self.add_pred(header, from);
        }

        self.seal_block(header);
        self.seal_block(exit_b);
        self.current = exit_b;
        Ok(())
    }

    /// `do body while (test)` — the body runs once before the test. The body is
    /// the loop entry and stays unsealed for the back-edge; the test lives in a
    /// dedicated latch block, which is where `continue` lands.
    pub(super) fn lower_do_while(&mut self, body: &[HirStmt], test: &HirExpr) -> Result<()> {
        let pre = self.current;
        let body_b = self.new_block();
        self.set_term(Terminator::Jump {
            target: body_b,
            args: Vec::new(),
        });
        self.add_pred(body_b, pre);
        // body_b NOT sealed: the back-edge from the latch is built below.
        let latch = self.new_block();
        let exit_b = self.new_block();

        self.loops.push(LoopCtx {
            continue_target: latch,
            break_target: exit_b,
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

        self.seal_block(latch); // body fall-through + any `continue` edges
        self.current = latch;
        let cond = self.lower_expr(test)?;
        self.set_term(Terminator::Branch {
            cond,
            then_blk: body_b,
            then_args: Vec::new(),
            else_blk: exit_b,
            else_args: Vec::new(),
        });
        self.add_pred(body_b, latch); // back-edge
        self.add_pred(exit_b, latch);
        self.seal_block(body_b); // pre + back-edge now present
        self.seal_block(exit_b); // latch's else + any `break` edges now present
        self.current = exit_b;
        Ok(())
    }

}
