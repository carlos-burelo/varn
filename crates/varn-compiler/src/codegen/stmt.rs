use super::class::{
    compile_class_decl, compile_enum_decl, compile_extension_decl, compile_namespace_decl,
    compile_sum_type,
};
use super::compiler::{Compiler, LoopCtx};
use super::expr::compile_expr;
use super::function::{
    compile_function, declare_pattern_global, declare_pattern_local, emit_closure,
};
use crate::chunk::Chunk;
use std::rc::Rc;
use varn_core::ast::*;
use varn_core::{MemberKey, OpCode};

pub fn compile_stmts<'a>(c: &mut Compiler<'a>, stmts: &[Stmt]) {
    for stmt in stmts {
        compile_stmt(c, stmt);
        if stmt_terminates(stmt) {
            break;
        }
    }
}

fn stmt_terminates(stmt: &Stmt) -> bool {
    match &stmt.kind {
        StmtKind::Return { .. } | StmtKind::Throw { .. } => true,
        StmtKind::Break { .. } | StmtKind::Continue { .. } => true,
        StmtKind::Block { stmts } => stmts.iter().any(stmt_terminates),
        StmtKind::If {
            consequent,
            alternate,
            ..
        } => alternate.as_ref().map_or(false, |alt| {
            stmt_terminates(consequent) && stmt_terminates(alt)
        }),
        _ => false,
    }
}

pub fn compile_stmt<'a>(c: &mut Compiler<'a>, stmt: &Stmt) {
    c.line = stmt.range().start.line;
    match &stmt.kind {
        StmtKind::Empty | StmtKind::Debugger => {}

        StmtKind::Expr { expression } => {
            let r = compile_expr(c, expression);

            let _ = r;
            c.free_reg();
        }

        StmtKind::Block { stmts } => {
            c.push_scope();
            compile_stmts(c, stmts);
            c.pop_scope();
        }

        StmtKind::Return { argument } => {
            let ret_reg = if let Some(val) = argument {
                compile_expr(c, val)
            } else {
                let r = c.alloc_reg();
                c.emit_rr(OpCode::LoadNull, r, 0);
                r
            };

            let fins = c.finally_stack.clone();
            for fin in fins.iter().rev() {
                c.emit(OpCode::PopTry);
                compile_stmt(c, fin);
            }
            let line = c.line;
            c.chunk
                .emit1(OpCode::Return, Chunk::pack(0, ret_reg) as u16, line);
        }

        StmtKind::Throw { argument } => {
            let r = compile_expr(c, argument);
            let line = c.line;
            c.chunk.emit1(OpCode::Throw, Chunk::pack(r, 0) as u16, line);
        }

        StmtKind::Break { .. } => {
            if let Some(depth) = c.loop_stack.last().map(|ctx| ctx.finally_depth) {
                let fins = c.finally_stack.clone();
                for i in (depth..fins.len()).rev() {
                    c.emit(OpCode::PopTry);
                    compile_stmt(c, &fins[i]);
                }
                let p = c.emit_jump(OpCode::Jump);
                c.loop_stack.last_mut().unwrap().break_jumps.push(p);
            }
        }

        StmtKind::Continue { .. } => {
            if let Some(depth) = c.loop_stack.last().map(|ctx| ctx.finally_depth) {
                let fins = c.finally_stack.clone();
                for i in (depth..fins.len()).rev() {
                    c.emit(OpCode::PopTry);
                    compile_stmt(c, &fins[i]);
                }
                let p = c.emit_jump(OpCode::Jump);
                c.loop_stack.last_mut().unwrap().continue_jumps.push(p);
            }
        }

        StmtKind::If {
            test,
            consequent,
            alternate,
        } => {
            let cond = compile_expr(c, test);
            let then_j = c.emit_cond_jump(OpCode::JumpIfFalse, cond);
            c.free_reg();
            compile_stmt(c, consequent);
            if let Some(alt) = alternate {
                let else_j = c.emit_jump(OpCode::Jump);
                c.patch_jump(then_j);
                compile_stmt(c, alt);
                c.patch_jump(else_j);
            } else {
                let over_j = c.emit_jump(OpCode::Jump);
                c.patch_jump(then_j);
                c.patch_jump(over_j);
            }
        }

        StmtKind::While { test, body } => {
            let loop_start = c.chunk.code.len();
            let cond = compile_expr(c, test);
            let exit_j = c.emit_cond_jump(OpCode::JumpIfFalse, cond);
            c.free_reg();
            c.loop_stack.push(LoopCtx {
                start: loop_start,
                break_jumps: vec![],
                continue_jumps: vec![],
                finally_depth: c.finally_stack.len(),
            });
            compile_stmt(c, body);
            let ctx = c.loop_stack.pop().unwrap();
            for p in ctx.continue_jumps {
                c.patch_jump(p);
            }
            c.emit_loop(loop_start);
            c.patch_jump(exit_j);
            for p in ctx.break_jumps {
                c.patch_jump(p);
            }
        }

        StmtKind::DoWhile { body, test } => {
            let loop_start = c.chunk.code.len();
            c.loop_stack.push(LoopCtx {
                start: loop_start,
                break_jumps: vec![],
                continue_jumps: vec![],
                finally_depth: c.finally_stack.len(),
            });
            compile_stmt(c, body);
            let ctx = c.loop_stack.pop().unwrap();
            for p in ctx.continue_jumps {
                c.patch_jump(p);
            }
            let cond = compile_expr(c, test);
            let exit_j = c.emit_cond_jump(OpCode::JumpIfFalse, cond);
            c.free_reg();
            c.emit_loop(loop_start);
            c.patch_jump(exit_j);
            for p in ctx.break_jumps {
                c.patch_jump(p);
            }
        }

        StmtKind::For {
            init,
            test,
            update,
            body,
        } => {
            c.push_scope();
            if let Some(init_box) = init {
                match &**init_box {
                    ForInit::Var { declarators, .. } => {
                        for d in declarators {
                            let r = if let Some(init_expr) = &d.init {
                                compile_expr(c, init_expr)
                            } else {
                                let r = c.alloc_reg();
                                c.emit_rr(OpCode::LoadNull, r, 0);
                                r
                            };
                            declare_pattern_local(c, &d.id, r);
                        }
                    }
                    ForInit::Expr(expr) => {
                        let r = compile_expr(c, expr);
                        c.free_reg();
                        let _ = r;
                    }
                }
            }
            let loop_start = c.chunk.code.len();
            let mut exit_j = None;
            if let Some(test_expr) = test {
                let cond = compile_expr(c, test_expr);
                let j = c.emit_cond_jump(OpCode::JumpIfFalse, cond);
                c.free_reg();
                exit_j = Some(j);
            }
            c.loop_stack.push(LoopCtx {
                start: loop_start,
                break_jumps: vec![],
                continue_jumps: vec![],
                finally_depth: c.finally_stack.len(),
            });
            compile_stmt(c, body);
            let ctx = c.loop_stack.pop().unwrap();
            for p in ctx.continue_jumps {
                c.patch_jump(p);
            }
            if let Some(update_expr) = update {
                let r = compile_expr(c, update_expr);
                c.free_reg();
                let _ = r;
            }
            c.emit_loop(loop_start);
            if let Some(j) = exit_j {
                c.patch_jump(j);
            }
            c.pop_scope();
            for p in ctx.break_jumps {
                c.patch_jump(p);
            }
        }

        StmtKind::ForIn {
            left, right, body, ..
        } => {
            c.push_scope();
            let obj = compile_expr(c, right);

            c.emit_rr(OpCode::ObjectKeys, obj, obj);
            let keys = obj;

            let idx = c.alloc_reg();
            c.emit_load_int(idx, 0);

            let len_reg = c.alloc_reg();
            let length_key = c.add_str(MemberKey::Length.as_str());

            let loop_start = c.chunk.code.len();

            c.emit_property(OpCode::GetProperty, len_reg, keys, length_key);

            let cond = c.alloc_reg();
            c.emit_rrr(OpCode::Lt, cond, idx, len_reg);
            let exit_j = c.emit_cond_jump(OpCode::JumpIfFalse, cond);
            c.free_reg();

            let elem = c.alloc_reg();
            c.emit_rrr(OpCode::GetIndex, elem, keys, idx);

            declare_pattern_local(c, left, elem);

            c.loop_stack.push(LoopCtx {
                start: loop_start,
                break_jumps: vec![],
                continue_jumps: vec![],
                finally_depth: c.finally_stack.len(),
            });
            compile_stmt(c, body);
            let ctx = c.loop_stack.pop().unwrap();
            for p in ctx.continue_jumps {
                c.patch_jump(p);
            }
            c.free_reg();

            let one = c.alloc_reg();
            c.emit_load_int(one, 1);
            c.emit_rrr(OpCode::Add, idx, idx, one);
            c.free_reg();

            c.emit_loop(loop_start);
            c.patch_jump(exit_j);
            for p in ctx.break_jumps {
                c.patch_jump(p);
            }
            c.pop_scope();
        }

        StmtKind::ForOf {
            left,
            right,
            body,
            is_await,
            ..
        } => {
            use varn_types::value::RuntimeSymbol;
            c.push_scope();

            let iterable = compile_expr(c, right);
            let sym_kind = if *is_await {
                RuntimeSymbol::AsyncIterator
            } else {
                RuntimeSymbol::Iterator
            };
            let sym_idx = c.chunk.add_symbol(sym_kind);

            let iter_fn = c.alloc_reg();
            let line = c.line;
            c.chunk
                .emit_rrc(OpCode::GetSymbol, iter_fn, iterable as u8, sym_idx, line);

            let iterator = c.alloc_reg();
            c.chunk.emit(OpCode::Call, line);
            c.chunk.write(Chunk::pack(iterator, iter_fn), line);
            c.chunk.write(Chunk::pack(1, iterable as u8), line);

            let loop_start = c.chunk.code.len();

            let next_fn = iter_fn;
            let next_key = c.add_str(MemberKey::IterNext.as_str());
            c.emit_property(OpCode::GetProperty, next_fn, iterator, next_key);

            let result_obj = c.alloc_reg();
            let line = c.line;
            c.chunk.emit(OpCode::Call, line);
            c.chunk.write(Chunk::pack(result_obj, next_fn), line);
            c.chunk.write(Chunk::pack(1, iterator as u8), line);

            let result_for_await = if *is_await {
                let awaited = c.alloc_reg();
                c.emit_rr(OpCode::Await, awaited, result_obj);

                awaited
            } else {
                result_obj
            };

            let done_key = c.add_str(MemberKey::IterDone.as_str());
            let done_reg = c.alloc_reg();
            c.emit_property(OpCode::GetProperty, done_reg, result_for_await, done_key);
            let exit_j = c.emit_cond_jump(OpCode::JumpIfTrue, done_reg);

            c.free_reg();

            let value_key = c.add_str(MemberKey::IterValue.as_str());
            let value_reg = c.alloc_reg();
            c.emit_property(OpCode::GetProperty, value_reg, result_for_await, value_key);

            declare_pattern_local(c, left, value_reg);

            c.loop_stack.push(LoopCtx {
                start: loop_start,
                break_jumps: vec![],
                continue_jumps: vec![],
                finally_depth: c.finally_stack.len(),
            });
            compile_stmt(c, body);
            let ctx = c.loop_stack.pop().unwrap();

            c.free_reg();

            if *is_await {
                c.free_reg();
            }
            c.free_reg();
            for p in ctx.continue_jumps {
                c.patch_jump(p);
            }

            c.emit_loop(loop_start);
            c.patch_jump(exit_j);

            // Free loop-internal registers before patching break jump targets so
            // regalloc sees their live ranges end before the break landing pads.
            c.free_reg(); // result_obj (or awaited)
            c.free_reg(); // iterator
            c.free_reg(); // iter_fn
            for p in ctx.break_jumps {
                c.patch_jump(p);
            }
            c.pop_scope();
        }

        StmtKind::Switch {
            discriminant,
            cases,
        } => {
            c.push_scope();
            let disc = compile_expr(c, discriminant);
            let mut next_jumps: Vec<usize> = vec![];
            let mut break_jumps: Vec<usize> = vec![];

            for case in cases {
                for p in next_jumps.drain(..) {
                    c.patch_jump(p);
                }
                if let Some(test) = &case.test {
                    let test_r = compile_expr(c, test);
                    let eq_r = c.alloc_reg();
                    c.emit_rrr(OpCode::Eq, eq_r, disc, test_r);
                    c.free_reg();
                    let skip = c.emit_cond_jump(OpCode::JumpIfFalse, eq_r);
                    c.free_reg();
                    next_jumps.push(skip);
                }
                c.loop_stack.push(LoopCtx {
                    start: 0,
                    break_jumps: vec![],
                    continue_jumps: vec![],
                    finally_depth: c.finally_stack.len(),
                });
                for s in &case.body {
                    compile_stmt(c, s);
                }
                let ctx = c.loop_stack.pop().unwrap();
                break_jumps.extend(ctx.break_jumps);
            }
            for p in next_jumps {
                c.patch_jump(p);
            }
            c.free_reg();
            for p in break_jumps {
                c.patch_jump(p);
            }
            c.pop_scope();
        }

        StmtKind::Try {
            block,
            catch,
            finally,
        } => {
            // Push finally onto stack so return/break/continue inside try body emit it.
            if let Some(fin) = finally {
                c.finally_stack.push(fin.as_ref().clone());
            }

            let err_reg = c.alloc_reg();
            let line = c.line;
            c.chunk.emit(OpCode::Try, line);
            c.chunk.write(crate::chunk::Chunk::pack(err_reg, 0), line);
            let try_patch = c.chunk.code.len();
            c.chunk.write(0xFFFF, line);
            c.chunk.write(0xFFFF, line);

            compile_stmt(c, block);

            // Pop finally AFTER try body so return/break inside try body see it.
            // Keep it on the stack while compiling catch (see below).
            c.emit(OpCode::PopTry);
            let after_try_j = c.emit_jump(OpCode::Jump);
            c.patch_jump(try_patch);

            if let Some(catch_clause) = catch {
                // Pop finally from stack now so catch body's return/break/continue
                // don't double-emit it. We protect the catch body with its own Try
                // so exceptions in catch still reach finally.
                if finally.is_some() {
                    c.finally_stack.pop();
                }

                if let Some(fin) = finally {
                    // Wrap catch body in Try so exceptions reach finally + rethrow.
                    let catch_err_reg = c.alloc_reg();
                    c.chunk.emit(OpCode::Try, line);
                    c.chunk.write(Chunk::pack(catch_err_reg, 0), line);
                    let catch_try_patch = c.chunk.code.len();
                    c.chunk.write(0xFFFF, line);
                    c.chunk.write(0xFFFF, line);

                    c.push_scope();
                    if let Some(param) = &catch_clause.param {
                        declare_pattern_local(c, param, err_reg);
                    }
                    compile_stmt(c, &catch_clause.body);
                    c.pop_scope();

                    c.emit(OpCode::PopTry);
                    let catch_finally_j = c.emit_jump(OpCode::Jump);
                    c.patch_jump(catch_try_patch);

                    // Exception path in catch: run finally then rethrow.
                    compile_stmt(c, fin);
                    c.chunk.emit(OpCode::Throw, line);
                    c.chunk
                        .write(Chunk::pack(catch_err_reg, catch_err_reg), line);

                    c.patch_jump(catch_finally_j);
                    c.free_reg(); // catch_err_reg
                } else {
                    c.push_scope();
                    if let Some(param) = &catch_clause.param {
                        declare_pattern_local(c, param, err_reg);
                    }
                    compile_stmt(c, &catch_clause.body);
                    c.pop_scope();
                }
            } else if finally.is_some() {
                c.finally_stack.pop();
            }

            c.free_reg(); // err_reg

            c.patch_jump(after_try_j);
            if let Some(fin) = finally {
                compile_stmt(c, fin);
            }
        }

        StmtKind::Using {
            declarations,
            is_await,
        } => {
            let depth = c.scopes.len();
            for d in declarations {
                let r = if let Some(init) = &d.init {
                    compile_expr(c, init)
                } else {
                    let r = c.alloc_reg();
                    c.emit_rr(OpCode::LoadNull, r, 0);
                    r
                };
                let name = match &d.id {
                    Pattern::Identifier { name, .. } => name.clone(),
                    _ => Rc::from("__using__"),
                };
                c.define_local(name, r);
                c.disposables.push((r, *is_await, depth));
            }
        }

        StmtKind::Labeled { body, .. } => {
            compile_stmt(c, body);
        }

        StmtKind::Decl(decl) => {
            compile_decl(c, decl);
        }
    }
}

pub fn compile_decl<'a>(c: &mut Compiler<'a>, decl: &Decl) {
    match decl {
        Decl::Variable(v) => {
            if !v.is_declare {
                compile_var_decl(c, v);
            }
        }
        Decl::Function(f) => {
            if !f.modifiers.is_declare {
                compile_fn_decl(c, f);
            }
        }
        Decl::Class(cl) => {
            if !cl.modifiers.is_declare {
                compile_class_decl(c, cl);
            }
        }
        Decl::Import(i) => compile_import(c, i),
        Decl::Export(e) => compile_export(c, e),
        Decl::Enum(en) => compile_enum_decl(c, en),
        Decl::Namespace(ns) => compile_namespace_decl(c, ns),
        Decl::Extension(ext) => compile_extension_decl(c, ext),
        Decl::SumType(st) => compile_sum_type(c, st),
        Decl::Interface(_) | Decl::TypeAlias(_) | Decl::Struct(_) => {}
    }
}

fn compile_var_decl<'a>(c: &mut Compiler<'a>, decl: &VariableDecl) {
    for d in &decl.declarators {
        let is_fn_init = d
            .init
            .as_ref()
            .map(|e| {
                matches!(
                    &e.kind,
                    varn_core::ast::ExprKind::Arrow { .. }
                        | varn_core::ast::ExprKind::Function { .. }
                )
            })
            .unwrap_or(false);
        let pre_reg = if !c.is_global && is_fn_init {
            if let varn_core::ast::Pattern::Identifier { name, .. } = &d.id {
                let r = c.alloc_reg();
                c.emit_rr(OpCode::LoadNull, r, 0);
                c.define_local(name.clone(), r);
                Some(r)
            } else {
                None
            }
        } else {
            None
        };

        let r = if let Some(init) = &d.init {
            compile_expr(c, init)
        } else {
            let r = c.alloc_reg();
            c.emit_rr(OpCode::LoadNull, r, 0);
            r
        };

        if c.is_global {
            declare_pattern_global(c, &d.id, r);
            c.free_reg();
        } else if let Some(pre) = pre_reg {
            if r != pre {
                c.emit_rr(OpCode::Move, pre, r);
                c.free_reg();
            }
        } else {
            declare_pattern_local(c, &d.id, r);
        }
    }
}

fn compile_fn_decl<'a>(c: &mut Compiler<'a>, decl: &FunctionDecl) {
    let (proto, upvalues) = compile_function(
        c,
        decl.id.clone(),
        &decl.params,
        &decl.body,
        decl.modifiers.is_async,
        decl.modifiers.is_generator,
        false,
    );
    let closure_reg = emit_closure(c, proto, upvalues);

    if c.is_global {
        let idx = c.add_str(&decl.id);
        let line = c.line;
        c.chunk
            .emit_rrc(OpCode::DefineGlobal, 0, closure_reg, idx, line);
        c.free_reg();
    } else {
        c.define_local(decl.id.clone(), closure_reg);
    }
}

fn compile_import<'a>(c: &mut Compiler<'a>, decl: &ImportDecl) {
    let src_idx = c.add_str(&decl.source);
    let mod_reg = c.alloc_reg();
    c.emit_rc(OpCode::LoadModule, mod_reg, src_idx);

    if decl.is_type {
        c.free_reg();
        return;
    }

    for spec in &decl.specifiers {
        match spec {
            ImportSpecifier::Default { local, range, .. } => {
                let dest = c.alloc_reg();
                if let Some(slot_idx) = c.annotations.get_slot_idx(range.start.offset) {
                    c.emit_rrc(OpCode::LoadModuleSlot, dest, mod_reg, slot_idx as u16);
                } else {
                    let key_idx = c.add_str("default");
                    c.emit_property(OpCode::GetProperty, dest, mod_reg, key_idx);
                }
                let local_idx = c.add_str(local);
                let line = c.line;
                c.chunk
                    .emit_rrc(OpCode::DefineGlobal, 0, dest, local_idx, line);
                c.free_reg();
            }
            ImportSpecifier::Named {
                local,
                imported,
                range,
                ..
            } => {
                let dest = c.alloc_reg();
                if let Some(slot_idx) = c.annotations.get_slot_idx(range.start.offset) {
                    c.emit_rrc(OpCode::LoadModuleSlot, dest, mod_reg, slot_idx as u16);
                } else {
                    let key_idx = c.add_str(imported);
                    c.emit_property(OpCode::GetProperty, dest, mod_reg, key_idx);
                }
                let local_idx = c.add_str(local);
                let line = c.line;
                c.chunk
                    .emit_rrc(OpCode::DefineGlobal, 0, dest, local_idx, line);
                c.free_reg();
            }
            ImportSpecifier::Namespace { local, .. } => {
                let local_idx = c.add_str(local);
                let line = c.line;
                c.chunk
                    .emit_rrc(OpCode::DefineGlobal, 0, mod_reg, local_idx, line);
            }
        }
    }
    c.free_reg();
}

fn compile_export<'a>(c: &mut Compiler<'a>, decl: &ExportDecl) {
    match decl {
        ExportDecl::Decl { declaration, .. } => {
            compile_decl(c, declaration);
            for name in runtime_export_names(declaration) {
                let val_reg = c.alloc_reg();
                let idx = c.add_str(&name);
                if !c.emit_load_var(&name, val_reg) {
                    c.emit_rc(OpCode::LoadGlobal, val_reg, idx);
                }
                if let Some(slot_idx) = c.annotations.get_slot_idx(declaration.range().start.offset)
                {
                    c.emit_rc(OpCode::StoreModuleSlot, val_reg, slot_idx as u16);
                }
                c.free_reg();
            }
        }
        ExportDecl::Default {
            declaration, range, ..
        } => match declaration.as_ref() {
            ExportDefaultDecl::Function(f) => {
                if !f.modifiers.is_declare {
                    compile_fn_decl(c, f);
                    let val_reg = c.alloc_reg();
                    let idx = c.add_str(&f.id);
                    if !c.emit_load_var(&f.id, val_reg) {
                        c.emit_rc(OpCode::LoadGlobal, val_reg, idx);
                    }
                    if let Some(slot_idx) = c.annotations.get_slot_idx(range.start.offset) {
                        c.emit_rc(OpCode::StoreModuleSlot, val_reg, slot_idx as u16);
                    }
                    c.free_reg();
                }
            }
            ExportDefaultDecl::Class(cl) => {
                if !cl.modifiers.is_declare {
                    compile_class_decl(c, cl);
                    if let Some(id) = &cl.id {
                        let val_reg = c.alloc_reg();
                        let idx = c.add_str(id);
                        if !c.emit_load_var(id, val_reg) {
                            c.emit_rc(OpCode::LoadGlobal, val_reg, idx);
                        }
                        if let Some(slot_idx) = c.annotations.get_slot_idx(range.start.offset) {
                            c.emit_rc(OpCode::StoreModuleSlot, val_reg, slot_idx as u16);
                        }
                        c.free_reg();
                    }
                }
            }
            ExportDefaultDecl::Expr(e) => {
                let r = compile_expr(c, e);
                if let Some(slot_idx) = c.annotations.get_slot_idx(range.start.offset) {
                    c.emit_rc(OpCode::StoreModuleSlot, r, slot_idx as u16);
                }
                c.free_reg();
                let _ = r;
            }
        },
        ExportDecl::Named {
            specifiers,
            source,
            range: _,
            ..
        } => match source {
            Some(src) if !specifiers.is_empty() => {
                let src_idx = c.add_str(src);
                let mod_reg = c.alloc_reg();
                c.emit_rc(OpCode::LoadModule, mod_reg, src_idx);
                for spec in specifiers {
                    let imported_idx = c.add_str(&spec.local);
                    let val_reg = c.alloc_reg();
                    if let Some(imported_slot) = c.annotations.get_slot_idx(spec.range.start.offset)
                    {
                        c.emit_rrc(
                            OpCode::LoadModuleSlot,
                            val_reg,
                            mod_reg,
                            imported_slot as u16,
                        );
                    } else {
                        c.emit_property(OpCode::GetProperty, val_reg, mod_reg, imported_idx);
                    }
                    if let Some(exported_slot) = c.annotations.get_slot_idx(spec.range.start.offset)
                    {
                        c.emit_rc(OpCode::StoreModuleSlot, val_reg, exported_slot as u16);
                    }
                    c.free_reg();
                }
                c.free_reg();
            }
            Some(_) => {}
            None => {
                for spec in specifiers {
                    let val_reg = c.alloc_reg();
                    let idx = c.add_str(&spec.local);
                    if !c.emit_load_var(&spec.local, val_reg) {
                        c.emit_rc(OpCode::LoadGlobal, val_reg, idx);
                    }
                    if let Some(exported_slot) = c.annotations.get_slot_idx(spec.range.start.offset)
                    {
                        c.emit_rc(OpCode::StoreModuleSlot, val_reg, exported_slot as u16);
                    }
                    c.free_reg();
                }
            }
        },
        ExportDecl::All {
            source,
            alias,
            range,
            ..
        } => {
            let src_idx = c.add_str(source);
            match alias {
                Some(_alias_name) => {
                    let mod_reg = c.alloc_reg();
                    c.emit_rc(OpCode::LoadModule, mod_reg, src_idx);
                    if let Some(slot_idx) = c.annotations.get_slot_idx(range.start.offset) {
                        c.emit_rc(OpCode::StoreModuleSlot, mod_reg, slot_idx as u16);
                    }
                    c.free_reg();
                }
                None => {
                    let mod_reg = c.alloc_reg();
                    c.emit_rc(OpCode::LoadModule, mod_reg, src_idx);
                    c.free_reg();
                }
            }
        }
    }
}

fn runtime_export_names(decl: &Decl) -> Vec<Rc<str>> {
    match decl {
        Decl::Variable(v) => {
            if v.is_declare {
                return vec![];
            }
            v.declarators
                .iter()
                .flat_map(|d| pattern_names(&d.id))
                .collect()
        }
        Decl::Function(f) => {
            if f.modifiers.is_declare {
                vec![]
            } else {
                vec![f.id.clone()]
            }
        }
        Decl::Class(c) => {
            if c.modifiers.is_declare {
                vec![]
            } else {
                c.id.as_ref().map(|id| id.clone()).into_iter().collect()
            }
        }
        Decl::Enum(e) => vec![e.id.clone()],
        Decl::Namespace(n) => vec![n.id.clone()],
        Decl::SumType(st) => st.variants.iter().map(|v| v.name.clone()).collect(),
        _ => vec![],
    }
}

fn pattern_names(pat: &Pattern) -> Vec<Rc<str>> {
    match pat {
        Pattern::Identifier { name, .. } => vec![name.clone()],
        Pattern::Array { elements, rest, .. } => {
            let mut v: Vec<Rc<str>> = elements
                .iter()
                .flatten()
                .flat_map(|el| pattern_names(&el.pattern))
                .collect();
            if let Some(r) = rest {
                v.extend(pattern_names(r));
            }
            v
        }
        Pattern::Object {
            properties, rest, ..
        } => {
            let mut v: Vec<Rc<str>> = properties
                .iter()
                .flat_map(|p| pattern_names(&p.value))
                .collect();
            if let Some(r) = rest {
                v.extend(pattern_names(r));
            }
            v
        }
        Pattern::Assignment { left, .. } => pattern_names(left),
        Pattern::Rest { argument, .. } => pattern_names(argument),
    }
}
