//! Statement lowering. `lower_stmt` dispatches; the shapes with real control
//! flow live in their own modules.

use crate::hir::{CaptureTarget, HirBinding, HirStmt};
use crate::ssa::ir::{InstKind, Terminator};
use crate::OptError;

use super::{Builder, Result, VarId};

mod branches;
mod loops;
mod modules;
mod try_catch;

impl Builder {
    pub(in crate::ssa::build) fn lower_block(&mut self, stmts: &[HirStmt]) -> Result<()> {
        for stmt in stmts {
            if !self.is_open() {
                break;
            }
            self.lower_stmt(stmt)?;
        }
        Ok(())
    }

    pub(in crate::ssa::build) fn lower_stmt(&mut self, stmt: &HirStmt) -> Result<()> {
        match stmt {
            HirStmt::Line(line) => {
                self.current_line = *line;
                Ok(())
            }
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
            HirStmt::SetFixedField {
                object,
                slot,
                value,
            } => {
                let o = self.lower_expr(object)?;
                let v = self.lower_expr(value)?;
                self.emit_effect(InstKind::SetFixedField {
                    object: o,
                    value: v,
                    slot: *slot,
                });
                Ok(())
            }
            HirStmt::SetIndex {
                object,
                index,
                value,
                is_array,
            } => {
                let o = self.lower_expr(object)?;
                let i = self.lower_expr(index)?;
                let v = self.lower_expr(value)?;
                if *is_array {
                    self.emit_effect(InstKind::ArraySetIndex {
                        object: o,
                        index: i,
                        value: v,
                    });
                } else {
                    self.emit_effect(InstKind::SetIndex {
                        object: o,
                        index: i,
                        value: v,
                    });
                }
                Ok(())
            }
            HirStmt::Return(value) => {
                let v = match value {
                    Some(e) => Some(self.lower_expr(e)?),
                    None => None,
                };
                self.emit_region_exits(0)?;
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
                self.emit_region_exits(loop_ctx.try_region_depth)?;
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
                self.emit_region_exits(loop_ctx.try_region_depth)?;
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
                catches,
                finally,
            } => self.lower_try(block, catches, finally),
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
}
