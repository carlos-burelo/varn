//! Statement lowering: `HirStmt -> bytecode`.

use varn_core::OpCode;
use varn_types::chunk::Chunk;

use super::FnLower;
use crate::hir::*;

impl FnLower {
    pub(super) fn lower_stmt(&mut self, stmt: &HirStmt) {
        match stmt {
            HirStmt::Expr(e) => {
                let mark = self.next_temp;
                let _ = self.lower_expr(e);
                self.free_to(mark);
            }
            HirStmt::Let { local, value, .. } => {
                let mark = self.next_temp;
                let v = self.lower_expr(value);
                let dst = self.local_reg(*local);
                self.chunk.emit_rr(OpCode::Move, dst, v, self.line);
                self.free_to(mark);
            }
            HirStmt::Assign { target, value } => {
                let mark = self.next_temp;
                let v = self.lower_expr(value);
                self.store_binding(target, v);
                self.free_to(mark);
            }
            HirStmt::SetMember {
                object,
                name,
                value,
            } => {
                let mark = self.next_temp;
                let obj = self.lower_expr(object);
                let val = self.lower_expr(value);
                let key_idx = self.chunk.add_str(name);
                // SetProperty obj.key = val (with an IC slot).
                self.emit_property(OpCode::SetProperty, obj, val, key_idx);
                self.free_to(mark);
            }
            HirStmt::SetIndex {
                object,
                index,
                value,
            } => {
                let mark = self.next_temp;
                let obj = self.lower_expr(object);
                let idx = self.lower_expr(index);
                let val = self.lower_expr(value);
                self.chunk
                    .emit_rrr(OpCode::SetIndex, obj, idx, val, self.line);
                self.free_to(mark);
            }
            HirStmt::Return(v) => {
                let mark = self.next_temp;
                let r = match v {
                    Some(e) => self.lower_expr(e),
                    None => {
                        let r = self.alloc();
                        self.chunk.emit_rr(OpCode::LoadNull, r, 0, self.line);
                        r
                    }
                };
                // Run every pending `finally` before leaving the function (the
                // return value is already computed and held below them).
                if !self.finally_stack.is_empty() {
                    self.run_finallys_from(0);
                }
                self.chunk
                    .emit1(OpCode::Return, Chunk::pack(0, r), self.line);
                self.free_to(mark);
            }
            HirStmt::Throw(e) => {
                let mark = self.next_temp;
                let r = self.lower_expr(e);
                self.chunk.emit1(OpCode::Throw, Chunk::pack(r, 0), self.line);
                self.free_to(mark);
            }
            HirStmt::Try {
                block,
                catch,
                finally,
            } => self.lower_try(block, catch, finally),
            HirStmt::If {
                test,
                then_body,
                else_body,
            } => {
                let mark = self.next_temp;
                let cond = self.lower_expr(test);
                let else_j = self.chunk.emit_cond_jump(OpCode::JumpIfFalse, cond, self.line);
                self.free_to(mark);
                for s in then_body {
                    self.lower_stmt(s);
                }
                if else_body.is_empty() {
                    self.chunk.patch_jump(else_j);
                } else {
                    let end_j = self.chunk.emit_jump(OpCode::Jump, self.line);
                    self.chunk.patch_jump(else_j);
                    for s in else_body {
                        self.lower_stmt(s);
                    }
                    self.chunk.patch_jump(end_j);
                }
            }
            HirStmt::While { test, body } => {
                let loop_start = self.chunk.code.len();
                let mark = self.next_temp;
                let cond = self.lower_expr(test);
                let exit = self.chunk.emit_cond_jump(OpCode::JumpIfFalse, cond, self.line);
                self.free_to(mark);
                self.loops.push(super::LoopCtx {
                    cont: super::ContinueMode::Backward(loop_start),
                    break_jumps: Vec::new(),
                    continue_jumps: Vec::new(),
                    finally_depth: self.finally_stack.len(),
                });
                for s in body {
                    self.lower_stmt(s);
                }
                self.chunk.emit_loop(loop_start, self.line);
                self.chunk.patch_jump(exit);
                let ctx = self.loops.pop().unwrap();
                for bj in ctx.break_jumps {
                    self.chunk.patch_jump(bj);
                }
            }
            HirStmt::ForClassic { test, update, body } => {
                self.lower_for_classic(test, update, body)
            }
            HirStmt::ForOf {
                var,
                iterable,
                body,
                is_await,
            } => self.lower_for_of(*var, iterable, body, *is_await),
            HirStmt::ForIn { var, object, body } => self.lower_for_in(*var, object, body),
            HirStmt::DoWhile { body, test } => self.lower_do_while(body, test),
            HirStmt::Switch { disc, cases } => self.lower_switch(disc, cases),
            HirStmt::Break => {
                // Run any `finally` handlers entered inside the innermost scope
                // before jumping out of it.
                if let Some(depth) = self.loops.last().map(|c| c.finally_depth) {
                    self.run_finallys_from(depth);
                }
                let j = self.chunk.emit_jump(OpCode::Jump, self.line);
                if let Some(ctx) = self.loops.last_mut() {
                    ctx.break_jumps.push(j);
                }
            }
            HirStmt::Continue => {
                // Continue targets the innermost *loop*; skip break-only scopes
                // (switch).
                if let Some(i) = self
                    .loops
                    .iter()
                    .rposition(|c| !matches!(c.cont, super::ContinueMode::Skip))
                {
                    self.run_finallys_from(self.loops[i].finally_depth);
                    match self.loops[i].cont {
                        super::ContinueMode::Backward(target) => {
                            self.chunk.emit_loop(target, self.line)
                        }
                        super::ContinueMode::Forward => {
                            let j = self.chunk.emit_jump(OpCode::Jump, self.line);
                            self.loops[i].continue_jumps.push(j);
                        }
                        super::ContinueMode::Skip => {}
                    }
                }
            }
            HirStmt::Import {
                source,
                is_type,
                specs,
            } => {
                let mark = self.next_temp;
                let src_idx = self.chunk.add_str(source);
                let mod_reg = self.alloc();
                self.chunk
                    .emit_rc(OpCode::LoadModule, mod_reg, src_idx, self.line);
                if !*is_type {
                    for spec in specs {
                        match &spec.kind {
                            HirImportKind::Namespace => {
                                let local_idx = self.chunk.add_str(&spec.local);
                                self.chunk.emit_rrc(
                                    OpCode::DefineGlobal,
                                    0,
                                    mod_reg,
                                    local_idx,
                                    self.line,
                                );
                            }
                            HirImportKind::Default | HirImportKind::Named(_) => {
                                let dest = self.alloc();
                                if let Some(slot) = spec.slot {
                                    self.chunk.emit_rrc(
                                        OpCode::LoadModuleSlot,
                                        dest,
                                        mod_reg,
                                        slot,
                                        self.line,
                                    );
                                } else {
                                    let key = match &spec.kind {
                                        HirImportKind::Named(n) => n.as_ref(),
                                        _ => "default",
                                    };
                                    let key_idx = self.chunk.add_str(key);
                                    self.emit_property(OpCode::GetProperty, dest, mod_reg, key_idx);
                                }
                                let local_idx = self.chunk.add_str(&spec.local);
                                self.chunk.emit_rrc(
                                    OpCode::DefineGlobal,
                                    0,
                                    dest,
                                    local_idx,
                                    self.line,
                                );
                                self.free_to(mod_reg as u32 + 1);
                            }
                        }
                    }
                }
                self.free_to(mark);
            }
            HirStmt::StoreExport { name, slot } => {
                let mark = self.next_temp;
                let r = self.alloc();
                let idx = self.chunk.add_str(name);
                self.chunk.emit_rc(OpCode::LoadGlobal, r, idx, self.line);
                self.chunk
                    .emit_rc(OpCode::StoreModuleSlot, r, *slot, self.line);
                self.free_to(mark);
            }
            HirStmt::ExportNamed { specifiers, source } => {
                let mark = self.next_temp;
                match source {
                    Some(src) => {
                        let src_idx = self.chunk.add_str(src);
                        let mod_reg = self.alloc();
                        self.chunk.emit_rc(OpCode::LoadModule, mod_reg, src_idx, self.line);
                        for spec in specifiers {
                            let val_reg = self.alloc();
                            if let Some(imported_slot) = spec.local_slot {
                                self.chunk.emit_rrc(
                                    OpCode::LoadModuleSlot,
                                    val_reg,
                                    mod_reg,
                                    imported_slot,
                                    self.line,
                                );
                            } else {
                                let imported_idx = self.chunk.add_str(&spec.local);
                                self.emit_property(OpCode::GetProperty, val_reg, mod_reg, imported_idx);
                            }
                            if let Some(exported_slot) = spec.exported_slot {
                                self.chunk.emit_rc(OpCode::StoreModuleSlot, val_reg, exported_slot, self.line);
                            }
                            self.free_to(val_reg as u32);
                        }
                    }
                    None => {
                        for spec in specifiers {
                            let val_reg = self.load_binding(&spec.binding);
                            if let Some(exported_slot) = spec.exported_slot {
                                self.chunk.emit_rc(OpCode::StoreModuleSlot, val_reg, exported_slot, self.line);
                            }
                            self.free_to(val_reg as u32);
                        }
                    }
                }
                self.free_to(mark);
            }
            HirStmt::ExportAll { source, alias, slot } => {
                let mark = self.next_temp;
                let src_idx = self.chunk.add_str(source);
                match alias {
                    Some(_) => {
                        let mod_reg = self.alloc();
                        self.chunk.emit_rc(OpCode::LoadModule, mod_reg, src_idx, self.line);
                        if let Some(slot_idx) = slot {
                            self.chunk.emit_rc(OpCode::StoreModuleSlot, mod_reg, *slot_idx, self.line);
                        }
                    }
                    None => {
                        let mod_reg = self.alloc();
                        self.chunk.emit_rc(OpCode::LoadModule, mod_reg, src_idx, self.line);
                    }
                }
                self.free_to(mark);
            }
            HirStmt::ExportDefaultExpr { value, slot } => {
                let mark = self.next_temp;
                let r = self.lower_expr(value);
                if let Some(slot_idx) = slot {
                    self.chunk.emit_rc(OpCode::StoreModuleSlot, r, *slot_idx, self.line);
                }
                self.free_to(mark);
            }
            HirStmt::Dispose { target, is_await } => {
                // `resource.dispose()` / `.disposeAsync()` — no args, IC slot in
                // the opcode's dest field (legacy `pop_scope`).
                let reg = self.local_reg(*target);
                let method = if *is_await { "disposeAsync" } else { "dispose" };
                let str_idx = self.chunk.add_str(method);
                let cs = self.alloc_cache() as u8;
                self.chunk
                    .write(Chunk::pack_op(OpCode::CallMethod, cs), self.line);
                self.chunk.write(Chunk::pack(reg, reg), self.line);
                self.chunk.write(str_idx, self.line);
                self.chunk.write(Chunk::pack(0u8, 0u8), self.line);
            }
            HirStmt::CloseUpvalues(targets) => {
                // Close open upvalues over the lowest captured slot, matching
                // legacy `pop_scope`. The VM closes everything at or above it.
                let lowest = targets
                    .iter()
                    .map(|t| match t {
                        CaptureTarget::Param(i) => self.param_reg(*i),
                        CaptureTarget::Local(id) => self.local_reg(*id),
                    })
                    .min()
                    .unwrap_or(0);
                self.chunk
                    .emit1(OpCode::CloseUpvalue, lowest as u16, self.line);
            }
        }
    }

    pub(super) fn store_binding(&mut self, target: &HirBinding, value_reg: u8) {
        match target {
            HirBinding::Param(i) => {
                let dst = self.param_reg(*i);
                self.chunk.emit_rr(OpCode::Move, dst, value_reg, self.line);
            }
            HirBinding::Local(id) => {
                let dst = self.local_reg(*id);
                self.chunk.emit_rr(OpCode::Move, dst, value_reg, self.line);
            }
            HirBinding::Global(name) => {
                let idx = self.chunk.add_str(name);
                self.chunk
                    .emit_rrc(OpCode::DefineGlobal, 0, value_reg, idx, self.line);
            }
            HirBinding::Upvalue(uv) => {
                // StoreUpvalue: uv in hi byte, src in lo (legacy `emit_store_var`).
                self.chunk.emit1(
                    OpCode::StoreUpvalue,
                    Chunk::pack(*uv as u8, value_reg),
                    self.line,
                );
            }
        }
    }
}
