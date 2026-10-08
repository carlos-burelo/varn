use super::context::{Builder, LoopCtx, Result};
use crate::hir::{HirType, LocalId};
use crate::ssa::ir::{InstKind, Terminator, VarId};
use crate::OptError;
use varn_tir::TirStmt;

fn stmt_offset(s: &TirStmt) -> Option<u32> {
    match s {
        TirStmt::Expr(e) => first_real_span(e),
        TirStmt::Let { init: Some(e), .. } => first_real_span(e),
        TirStmt::Return(Some(e)) => first_real_span(e),
        TirStmt::Throw(e) => first_real_span(e),
        TirStmt::If { cond, .. } => first_real_span(cond),
        TirStmt::Loop { cond, .. } => first_real_span(cond),
        TirStmt::Try { body, .. } => body.first().and_then(stmt_offset),
        TirStmt::Let { .. }
        | TirStmt::Return(None)
        | TirStmt::Break
        | TirStmt::Continue
        | TirStmt::BuildClass(_) => None,
    }
}

fn first_real_span(e: &varn_tir::TirExpr) -> Option<u32> {
    if e.span.end > 0 {
        return Some(e.span.start);
    }
    crate::from_tir::tir_children::child_exprs(e)
        .into_iter()
        .find_map(first_real_span)
}

impl<'m> Builder<'m> {
    pub(super) fn lower_block(&mut self, stmts: &[TirStmt]) -> Result<()> {
        for s in stmts {
            if !self.is_open() {
                break;
            }
            self.lower_stmt(s)?;
        }
        Ok(())
    }

    pub(super) fn lower_seq(
        &mut self,
        stmts: &[TirStmt],
        value: &varn_tir::TirExpr,
    ) -> Result<crate::ssa::ir::Value> {
        self.lower_block(stmts)?;
        if !self.is_open() {
            let dead = self.new_block();
            self.seal_block(dead);
            self.current = dead;
        }
        self.lower_expr(value)
    }

    pub(super) fn lower_stmt(&mut self, s: &TirStmt) -> Result<()> {
        if let Some(offset) = stmt_offset(s) {
            self.set_line_at(offset);
        }
        match s {
            TirStmt::Expr(e) => {
                self.lower_expr(e)?;
            }
            TirStmt::Let { local, ty, init } => {
                let declared = self.ty(*ty);
                let var = VarId::Local(LocalId(local.0));
                self.var_ty.insert(var, declared);
                let value = match init {
                    Some(e) => {
                        let v = self.lower_expr(e)?;
                        let v = self.widen_exact(v, e.ty, *ty);
                        self.coerce(v, declared)
                    }
                    None => match declared {
                        HirType::Int => self.emit(InstKind::ConstInt(0), declared),
                        HirType::Float => self.emit(InstKind::ConstFloat(0.0), declared),
                        HirType::Bool => self.emit(InstKind::ConstBool(false), declared),
                        HirType::Str
                        | HirType::Ref
                        | HirType::Dynamic
                        | HirType::Array(_)
                        | HirType::Map(..)
                        | HirType::Set(_)
                        | HirType::Class(_)
                        | HirType::Nullable(_) => self.emit(InstKind::ConstNull, declared),
                    },
                };
                self.store_var(var, value);
            }
            TirStmt::Return(v) => {
                let val = match v {
                    Some(e) => {
                        let v = self.lower_expr(e)?;
                        Some(match self.return_bt {
                            Some(rt) => self.widen_exact(v, e.ty, rt),
                            None => v,
                        })
                    }
                    None => None,
                };
                for _ in 0..self.try_depth {
                    self.emit_effect(InstKind::PopTry);
                }
                self.set_term(Terminator::Return(val));
            }
            TirStmt::Throw(e) => {
                let v = self.lower_expr(e)?;
                self.set_term(Terminator::Throw(v));
            }
            TirStmt::Break => {
                if let Some(c) = self.loops.last().copied() {
                    for _ in 0..self.try_depth.saturating_sub(c.try_depth) {
                        self.emit_effect(InstKind::PopTry);
                    }
                    let from = self.current;
                    self.set_term(Terminator::Jump {
                        target: c.break_target,
                        args: vec![],
                    });
                    self.add_pred(c.break_target, from);
                }
            }
            TirStmt::Continue => {
                if let Some(c) = self.loops.last().copied() {
                    for _ in 0..self.try_depth.saturating_sub(c.try_depth) {
                        self.emit_effect(InstKind::PopTry);
                    }
                    let from = self.current;
                    self.set_term(Terminator::Jump {
                        target: c.continue_target,
                        args: vec![],
                    });
                    self.add_pred(c.continue_target, from);
                }
            }
            TirStmt::If {
                cond,
                then_body,
                else_body,
            } => {
                let c = self.lower_expr(cond)?;
                let then_blk = self.new_block();
                let else_blk = self.new_block();
                let join = self.new_block();
                let from = self.current;
                self.set_term(Terminator::Branch {
                    cond: c,
                    then_blk,
                    then_args: vec![],
                    else_blk,
                    else_args: vec![],
                });
                self.add_pred(then_blk, from);
                self.add_pred(else_blk, from);
                self.seal_block(then_blk);
                self.seal_block(else_blk);

                self.current = then_blk;
                self.lower_block(then_body)?;
                if self.is_open() {
                    let cur = self.current;
                    self.set_term(Terminator::Jump {
                        target: join,
                        args: vec![],
                    });
                    self.add_pred(join, cur);
                }

                self.current = else_blk;
                self.lower_block(else_body)?;
                if self.is_open() {
                    let cur = self.current;
                    self.set_term(Terminator::Jump {
                        target: join,
                        args: vec![],
                    });
                    self.add_pred(join, cur);
                }

                self.seal_block(join);
                self.current = join;
            }
            TirStmt::Loop { cond, body } => {
                let head = self.new_block();
                let body_blk = self.new_block();
                let exit = self.new_block();
                let from = self.current;
                self.set_term(Terminator::Jump {
                    target: head,
                    args: vec![],
                });
                self.add_pred(head, from);

                self.current = head;
                let c = self.lower_expr(cond)?;
                self.set_term(Terminator::Branch {
                    cond: c,
                    then_blk: body_blk,
                    then_args: vec![],
                    else_blk: exit,
                    else_args: vec![],
                });
                self.add_pred(body_blk, head);
                self.add_pred(exit, head);
                self.seal_block(body_blk);

                self.loops.push(LoopCtx {
                    continue_target: head,
                    break_target: exit,
                    try_depth: self.try_depth,
                });
                self.current = body_blk;
                self.lower_block(body)?;
                if self.is_open() {
                    let closes = self.loop_body_pinned(body);
                    if !closes.is_empty() {
                        self.emit_effect(InstKind::CloseUpvalues { targets: closes });
                    }
                    let cur = self.current;
                    self.set_term(Terminator::Jump {
                        target: head,
                        args: vec![],
                    });
                    self.add_pred(head, cur);
                }
                self.loops.pop();

                self.seal_block(head);
                self.seal_block(exit);
                self.current = exit;
            }
            TirStmt::Try {
                body,
                catch_local,
                catch_body,
            } => {
                let try_entry = self.current;
                let landing = self.new_block();
                let exit = self.new_block();

                let try_val = self.emit(InstKind::Try { handler: landing }, HirType::Dynamic);

                self.try_depth += 1;
                self.lower_block(body)?;
                self.try_depth -= 1;
                if self.is_open() {
                    self.emit_effect(InstKind::PopTry);
                    let from = self.current;
                    self.set_term(Terminator::Jump {
                        target: exit,
                        args: vec![],
                    });
                    self.add_pred(exit, from);
                }

                self.add_pred(landing, try_entry);
                self.seal_block(landing);
                self.current = landing;
                let err = self.emit(InstKind::CatchParam { try_val }, HirType::Dynamic);
                self.store_var(VarId::Local(LocalId(catch_local.0)), err);
                self.lower_block(catch_body)?;
                if self.is_open() {
                    let from = self.current;
                    self.set_term(Terminator::Jump {
                        target: exit,
                        args: vec![],
                    });
                    self.add_pred(exit, from);
                }

                self.seal_block(exit);
                self.current = exit;
            }
            TirStmt::BuildClass(n) => {
                let def = self
                    .tir
                    .class_defs
                    .get(*n as usize)
                    .ok_or(OptError::Unsupported("from_tir: BuildClass index"))?
                    .clone();
                self.build_class_def(&def)?;
            }
        }
        Ok(())
    }
}
