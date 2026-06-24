use std::rc::Rc;

use crate::hir::{
    CaptureTarget, HirBinOp, HirBinding, HirCatch, HirExportSpec, HirExpr, HirImportKind,
    HirImportSpec, HirStmt, HirSwitchCase, HirType, LocalId,
};
use crate::ssa::ir::{BlockId, InstKind, Terminator, Value};
use crate::OptError;

use super::{Builder, LoopCtx, Result, VarId};

impl Builder {
    pub(super) fn lower_block(&mut self, stmts: &[HirStmt]) -> Result<()> {
        for stmt in stmts {
            if !self.is_open() {
                break;
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

                self.store_binding(&HirBinding::Local(*local), v);
                Ok(())
            }
            HirStmt::Assign { target, value } => {
                let v = self.lower_expr(value)?;
                self.store_binding(target, v);
                Ok(())
            }
            HirStmt::SetMember {
                object,
                name,
                value,
            } => {
                let o = self.lower_expr(object)?;
                let v = self.lower_expr(value)?;
                self.emit_effect(InstKind::SetProperty {
                    object: o,
                    name: name.clone(),
                    value: v,
                });
                Ok(())
            }
            HirStmt::SetIndex {
                object,
                index,
                value,
            } => {
                let o = self.lower_expr(object)?;
                let i = self.lower_expr(index)?;
                let v = self.lower_expr(value)?;
                self.emit_effect(InstKind::SetIndex {
                    object: o,
                    index: i,
                    value: v,
                });
                Ok(())
            }
            HirStmt::Return(value) => {
                let v = match value {
                    Some(e) => Some(self.lower_expr(e)?),
                    None => None,
                };
                self.emit_exiting_finallys(0)?;
                if self.is_open() {
                    self.set_term(Terminator::Return(v));
                }
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
            HirStmt::ForClassic { test, update, body } => {
                self.lower_for_classic(test, update, body)
            }
            HirStmt::DoWhile { body, test } => self.lower_do_while(body, test),
            HirStmt::ForIn { var, object, body } => self.lower_for_in(*var, object, body),
            HirStmt::ForOf {
                var,
                iterable,
                body,
                is_await,
            } => self.lower_for_of(*var, iterable, body, *is_await),
            HirStmt::Switch { disc, cases } => self.lower_switch(disc, cases),
            HirStmt::Break => {
                let loop_ctx = self
                    .loops
                    .last()
                    .ok_or(OptError::Unsupported("ssa: break outside loop"))?
                    .clone();
                self.emit_exiting_finallys(loop_ctx.finally_depth)?;
                if self.is_open() {
                    let from = self.current;
                    self.set_term(Terminator::Jump {
                        target: loop_ctx.break_target,
                        args: Vec::new(),
                    });
                    self.add_pred(loop_ctx.break_target, from);
                }
                Ok(())
            }
            HirStmt::Continue => {
                let loop_ctx = self
                    .loops
                    .last()
                    .ok_or(OptError::Unsupported("ssa: continue outside loop"))?
                    .clone();
                self.emit_exiting_finallys(loop_ctx.finally_depth)?;
                if self.is_open() {
                    let from = self.current;
                    self.set_term(Terminator::Jump {
                        target: loop_ctx.continue_target,
                        args: Vec::new(),
                    });
                    self.add_pred(loop_ctx.continue_target, from);
                }
                Ok(())
            }
            HirStmt::Try {
                block,
                catch,
                finally,
            } => self.lower_try(block, catch, finally),
            HirStmt::CloseUpvalues(targets) => {
                let vars = targets
                    .iter()
                    .map(|t| match t {
                        CaptureTarget::Param(i) => VarId::Param(*i),
                        CaptureTarget::Local(id) => VarId::Local(*id),
                    })
                    .collect();
                self.emit_effect(InstKind::CloseUpvalues { targets: vars });
                Ok(())
            }
            HirStmt::Dispose { target, is_await } => {
                self.emit_effect(InstKind::Dispose {
                    target: *target,
                    is_await: *is_await,
                });
                Ok(())
            }
            HirStmt::Import {
                source,
                is_type,
                specs,
            } => self.lower_import(source, *is_type, specs),
            HirStmt::StoreExport { name, slot } => self.lower_store_export(name, *slot),
            HirStmt::ExportNamed { specifiers, source } => {
                self.lower_export_named(specifiers, source)
            }
            HirStmt::ExportAll {
                source,
                alias,
                slot,
            } => self.lower_export_all(source, alias, slot),
            HirStmt::ExportDefaultExpr { value, slot } => {
                self.lower_export_default_expr(value, slot)
            }
        }
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

    pub(super) fn lower_while(&mut self, test: &HirExpr, body: &[HirStmt]) -> Result<()> {
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
            continue_target: header,
            break_target: exit_b,
            finally_depth: self.finally_stack.len(),
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
            finally_depth: self.finally_stack.len(),
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

    pub(super) fn lower_switch(&mut self, disc: &HirExpr, cases: &[HirSwitchCase]) -> Result<()> {
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
            finally_depth: self.finally_stack.len(),
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

    pub(super) fn lower_for_of(
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
            finally_depth: self.finally_stack.len(),
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

    pub(super) fn lower_for_in(
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
                name: Rc::from("length"),
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
            finally_depth: self.finally_stack.len(),
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

    pub(super) fn lower_do_while(&mut self, body: &[HirStmt], test: &HirExpr) -> Result<()> {
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
            finally_depth: self.finally_stack.len(),
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
        self.set_term(Terminator::Branch {
            cond,
            then_blk: body_b,
            then_args: Vec::new(),
            else_blk: exit_b,
            else_args: Vec::new(),
        });
        self.add_pred(body_b, latch);
        self.add_pred(exit_b, latch);
        self.seal_block(body_b);
        self.seal_block(exit_b);
        self.current = exit_b;
        Ok(())
    }

    fn lower_try(
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

    fn lower_try_catch(&mut self, block: &[HirStmt], cc: &HirCatch) -> Result<()> {
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

    fn lower_try_finally(&mut self, block: &[HirStmt], fin: &[HirStmt]) -> Result<()> {
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

    fn lower_try_catch_finally(
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

    fn lower_import(
        &mut self,
        source: &Rc<str>,
        is_type: bool,
        specs: &[HirImportSpec],
    ) -> Result<()> {
        let mod_val = self.emit(
            InstKind::LoadModule {
                source: source.clone(),
            },
            HirType::Ref,
        );
        if !is_type {
            for spec in specs {
                match &spec.kind {
                    HirImportKind::Namespace => {
                        self.emit_effect(InstKind::StoreGlobal {
                            name: spec.local.clone(),
                            value: mod_val,
                        });
                    }
                    HirImportKind::Default | HirImportKind::Named(_) => {
                        let val = if let Some(slot) = spec.slot {
                            self.emit(
                                InstKind::ModuleSlot {
                                    object: mod_val,
                                    slot,
                                },
                                HirType::Dynamic,
                            )
                        } else {
                            let key = match &spec.kind {
                                HirImportKind::Named(n) => n.clone(),
                                _ => Rc::from("default"),
                            };
                            self.emit(
                                InstKind::GetProperty {
                                    object: mod_val,
                                    name: key,
                                },
                                HirType::Dynamic,
                            )
                        };
                        self.emit_effect(InstKind::StoreGlobal {
                            name: spec.local.clone(),
                            value: val,
                        });
                    }
                }
            }
        }
        Ok(())
    }

    fn lower_store_export(&mut self, name: &Rc<str>, slot: u16) -> Result<()> {
        let val = self.emit(InstKind::LoadGlobal(name.clone()), HirType::Dynamic);
        self.emit_effect(InstKind::StoreModuleSlot { value: val, slot });
        Ok(())
    }

    fn lower_export_named(
        &mut self,
        specifiers: &[HirExportSpec],
        source: &Option<Rc<str>>,
    ) -> Result<()> {
        match source {
            Some(src) => {
                let mod_val = self.emit(
                    InstKind::LoadModule {
                        source: src.clone(),
                    },
                    HirType::Ref,
                );
                for spec in specifiers {
                    let val = if let Some(imported_slot) = spec.local_slot {
                        self.emit(
                            InstKind::ModuleSlot {
                                object: mod_val,
                                slot: imported_slot,
                            },
                            HirType::Dynamic,
                        )
                    } else {
                        self.emit(
                            InstKind::GetProperty {
                                object: mod_val,
                                name: spec.local.clone(),
                            },
                            HirType::Dynamic,
                        )
                    };
                    if let Some(exported_slot) = spec.exported_slot {
                        self.emit_effect(InstKind::StoreModuleSlot {
                            value: val,
                            slot: exported_slot,
                        });
                    }
                }
            }
            None => {
                for spec in specifiers {
                    let val = self.load_binding(&spec.binding)?;
                    if let Some(exported_slot) = spec.exported_slot {
                        self.emit_effect(InstKind::StoreModuleSlot {
                            value: val,
                            slot: exported_slot,
                        });
                    }
                }
            }
        }
        Ok(())
    }

    fn lower_export_all(
        &mut self,
        source: &Rc<str>,
        alias: &Option<Rc<str>>,
        slot: &Option<u16>,
    ) -> Result<()> {
        let mod_val = self.emit(
            InstKind::LoadModule {
                source: source.clone(),
            },
            HirType::Ref,
        );
        if alias.is_some() {
            if let Some(slot_idx) = slot {
                self.emit_effect(InstKind::StoreModuleSlot {
                    value: mod_val,
                    slot: *slot_idx,
                });
            }
        }
        Ok(())
    }

    fn lower_export_default_expr(&mut self, value: &HirExpr, slot: &Option<u16>) -> Result<()> {
        let val = self.lower_expr(value)?;
        if let Some(slot_idx) = slot {
            self.emit_effect(InstKind::StoreModuleSlot {
                value: val,
                slot: *slot_idx,
            });
        }
        Ok(())
    }

    fn emit_exiting_finallys(&mut self, depth: usize) -> Result<()> {
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
