//! Statement AST→HIR lowering.

use varn_core::ast::operators::AssignOp;
use varn_core::ast::{Decl, ExprKind, ForInit, Pattern, Stmt, StmtKind};

use super::*;

impl<'a> Lowerer<'a> {
    /// Lower a statement that introduces its own block scope, returning the
    /// lowered body (with a trailing `CloseUpvalues` if the block captured any
    /// bindings).
    fn lower_block(&mut self, stmt: &Stmt, scope: &mut Scope) -> R<Vec<HirStmt>> {
        let mut out = Vec::new();
        scope.push_block();
        match &stmt.kind {
            StmtKind::Block { stmts } => {
                for s in stmts {
                    self.lower_stmt(s, scope, &mut out)?;
                }
            }
            _ => self.lower_stmt(stmt, scope, &mut out)?,
        }
        let (captured, disposables) = scope.pop_block();
        block_epilogue(&mut out, captured, disposables);
        Ok(out)
    }

    /// Lower a loop/clause body (a block or single statement) into `out` within
    /// the *current*, already-pushed block scope — so a loop variable declared
    /// in that block is visible to the body.
    fn lower_body_into(
        &mut self,
        body: &Stmt,
        scope: &mut Scope,
        out: &mut Vec<HirStmt>,
    ) -> R<()> {
        match &body.kind {
            StmtKind::Block { stmts } => {
                for s in stmts {
                    self.lower_stmt(s, scope, out)?;
                }
            }
            _ => self.lower_stmt(body, scope, out)?,
        }
        Ok(())
    }

    pub(super) fn lower_stmt(
        &mut self,
        stmt: &Stmt,
        scope: &mut Scope,
        out: &mut Vec<HirStmt>,
    ) -> R<()> {
        match &stmt.kind {
            StmtKind::Empty => {}
            StmtKind::Block { stmts } => {
                scope.push_block();
                for s in stmts {
                    self.lower_stmt(s, scope, out)?;
                }
                let (captured, disposables) = scope.pop_block();
                block_epilogue(out, captured, disposables);
            }
            StmtKind::Expr { expression } => {
                if let ExprKind::Assign { op, target, value } = &expression.kind {
                    // Member/index assignment target (simple `=` only).
                    if let ExprKind::Member {
                        object,
                        property,
                        computed,
                        optional,
                    } = &target.kind
                    {
                        if *optional {
                            return unsupported("optional assignment target");
                        }
                        if !matches!(op, AssignOp::Assign) {
                            return unsupported("compound member assignment");
                        }
                        let off = target.range.start.offset;
                        if self.ann.get_slot_idx(off).is_some() {
                            return unsupported("module-slot assignment");
                        }
                        if self.extension_set_members.contains_key(&off) {
                            return unsupported("extension setter");
                        }
                        if matches!(object.kind, ExprKind::Super) {
                            return unsupported("super assignment");
                        }
                        let object_hir = self.lower_expr(object, scope)?;
                        if *computed {
                            let index = self.lower_expr(property, scope)?;
                            let value = self.lower_expr(value, scope)?;
                            out.push(HirStmt::SetIndex {
                                object: object_hir,
                                index,
                                value,
                            });
                        } else {
                            let name = match &property.kind {
                                ExprKind::Identifier { name } => name.clone(),
                                _ => return unsupported("non-identifier property assign"),
                            };
                            let value = self.lower_expr(value, scope)?;
                            out.push(HirStmt::SetMember {
                                object: object_hir,
                                name,
                                value,
                            });
                        }
                        return Ok(());
                    }
                    let binding = match &target.kind {
                        ExprKind::Identifier { name } => self.resolve(name, scope),
                        _ => return unsupported("non-identifier assign target"),
                    };
                    let val_expr = self.lower_expr(value, scope)?;
                    let value = match op {
                        AssignOp::Assign => val_expr,
                        _ => {
                            let bop = compound_to_bin(*op)?;
                            let ty = numeric_ty(self.ann, expression.range.start.offset);
                            HirExpr::Binary {
                                op: bop,
                                lhs: Box::new(HirExpr::Var(binding.clone())),
                                rhs: Box::new(val_expr),
                                ty,
                            }
                        }
                    };
                    out.push(HirStmt::Assign {
                        target: binding,
                        value,
                    });
                } else {
                    let e = self.lower_expr(expression, scope)?;
                    out.push(HirStmt::Expr(e));
                }
            }
            StmtKind::Decl(decl) => match decl.as_ref() {
                Decl::Variable(v) => {
                    for d in &v.declarators {
                        let name = match &d.id {
                            Pattern::Identifier { name, .. } => name.clone(),
                            _ => return unsupported("destructuring let"),
                        };
                        let value = match &d.init {
                            Some(e) => self.lower_expr(e, scope)?,
                            None => HirExpr::Null,
                        };
                        let local = scope.alloc_local(name);
                        out.push(HirStmt::Let {
                            local,
                            value,
                            ty: HirType::Dynamic,
                        });
                    }
                }
                // Nested function declaration → closure bound to a local. Bind
                // the name *before* lowering the body so the body can capture
                // itself (recursion via an open upvalue on this local slot).
                Decl::Function(f) => {
                    let local = scope.alloc_local(f.id.clone());
                    let (func, upvalues) = self.lower_function(f, scope)?;
                    out.push(HirStmt::Let {
                        local,
                        value: HirExpr::Closure {
                            func: Box::new(func),
                            upvalues,
                        },
                        ty: HirType::Dynamic,
                    });
                }
                Decl::Class(cl) => {
                    let cname = match &cl.id {
                        Some(id) => id.clone(),
                        None => return unsupported("anonymous nested class"),
                    };
                    let hir_class = self.lower_class(cl, scope)?;
                    let local = scope.alloc_local(cname);
                    out.push(HirStmt::Let {
                        local,
                        value: HirExpr::Class(Box::new(hir_class)),
                        ty: HirType::Dynamic,
                    });
                }
                Decl::Enum(en) => {
                    let hir_enum = self.lower_enum(en, scope)?;
                    let local = scope.alloc_local(en.id.clone());
                    out.push(HirStmt::Let {
                        local,
                        value: HirExpr::Enum(Box::new(hir_enum)),
                        ty: HirType::Dynamic,
                    });
                }
                Decl::Interface(_) | Decl::TypeAlias(_) | Decl::Struct(_) => {}
                _ => return unsupported("nested namespace decl"),
            },
            StmtKind::Return { argument } => {
                let v = match argument {
                    Some(e) => Some(self.lower_expr(e, scope)?),
                    None => None,
                };
                out.push(HirStmt::Return(v));
            }
            StmtKind::If {
                test,
                consequent,
                alternate,
            } => {
                let test = self.lower_expr(test, scope)?;
                let then_body = self.lower_block(consequent, scope)?;
                let else_body = match alternate {
                    Some(alt) => self.lower_block(alt, scope)?,
                    None => Vec::new(),
                };
                out.push(HirStmt::If {
                    test,
                    then_body,
                    else_body,
                });
            }
            StmtKind::While { test, body } => {
                let test = self.lower_expr(test, scope)?;
                let body = self.lower_block(body, scope)?;
                out.push(HirStmt::While { test, body });
            }
            StmtKind::For {
                init,
                test,
                update,
                body,
            } => {
                // Desugar `for (init; test; update) body`
                //   -> init; while (test) { body; update }
                scope.push_block();
                if let Some(init) = init {
                    match init.as_ref() {
                        ForInit::Expr(e) => {
                            let e = self.lower_expr(e, scope)?;
                            out.push(HirStmt::Expr(e));
                        }
                        ForInit::Var { declarators, .. } => {
                            for d in declarators {
                                let name = match &d.id {
                                    Pattern::Identifier { name, .. } => name.clone(),
                                    _ => return unsupported("for-init destructuring"),
                                };
                                let value = match &d.init {
                                    Some(e) => self.lower_expr(e, scope)?,
                                    None => HirExpr::Null,
                                };
                                let local = scope.alloc_local(name);
                                out.push(HirStmt::Let {
                                    local,
                                    value,
                                    ty: HirType::Dynamic,
                                });
                            }
                        }
                    }
                }
                let test = match test {
                    Some(t) => self.lower_expr(t, scope)?,
                    None => HirExpr::Bool(true),
                };
                // `continue` must run `update` (not skip it), so the update is a
                // dedicated field rather than appended to the body.
                let body = self.lower_block(body, scope)?;
                let update = match update {
                    Some(u) => vec![HirStmt::Expr(self.lower_expr(u, scope)?)],
                    None => Vec::new(),
                };
                out.push(HirStmt::ForClassic { test, update, body });
                let (captured, disposables) = scope.pop_block();
                block_epilogue(out, captured, disposables);
            }
            StmtKind::ForOf {
                left,
                right,
                body,
                is_await,
                ..
            } => {
                if *is_await {
                    return unsupported("for-await-of");
                }
                // The iterable is evaluated in the enclosing scope (it cannot
                // reference the loop variable), which is then bound per-iteration
                // in the body's block.
                let iterable = self.lower_expr(right, scope)?;
                let name = match left {
                    Pattern::Identifier { name, .. } => name.clone(),
                    _ => return unsupported("for-of destructuring binding"),
                };
                scope.push_block();
                let var = scope.alloc_local(name);
                let mut hbody = Vec::new();
                if let Err(e) = self.lower_body_into(body, scope, &mut hbody) {
                    scope.pop_block();
                    return Err(e);
                }
                let (captured, disposables) = scope.pop_block();
                block_epilogue(&mut hbody, captured, disposables);
                out.push(HirStmt::ForOf {
                    var,
                    iterable,
                    body: hbody,
                });
            }
            StmtKind::ForIn {
                left, right, body, ..
            } => {
                let object = self.lower_expr(right, scope)?;
                let name = match left {
                    Pattern::Identifier { name, .. } => name.clone(),
                    _ => return unsupported("for-in destructuring binding"),
                };
                scope.push_block();
                let var = scope.alloc_local(name);
                let mut hbody = Vec::new();
                if let Err(e) = self.lower_body_into(body, scope, &mut hbody) {
                    scope.pop_block();
                    return Err(e);
                }
                let (captured, disposables) = scope.pop_block();
                block_epilogue(&mut hbody, captured, disposables);
                out.push(HirStmt::ForIn {
                    var,
                    object,
                    body: hbody,
                });
            }
            StmtKind::DoWhile { body, test } => {
                // Body's block-scoped locals are not visible to the trailing
                // test, so the test is lowered after the body's block closes.
                let hbody = self.lower_block(body, scope)?;
                let test = self.lower_expr(test, scope)?;
                out.push(HirStmt::DoWhile { body: hbody, test });
            }
            StmtKind::Switch {
                discriminant,
                cases,
            } => {
                let disc = self.lower_expr(discriminant, scope)?;
                // All cases share one block scope (JS switch-block semantics).
                scope.push_block();
                let mut hcases = Vec::new();
                let mut err = None;
                'cases: for case in cases {
                    let test = match &case.test {
                        Some(t) => match self.lower_expr(t, scope) {
                            Ok(v) => Some(v),
                            Err(e) => {
                                err = Some(e);
                                break 'cases;
                            }
                        },
                        None => None,
                    };
                    let mut body = Vec::new();
                    for s in &case.body {
                        if let Err(e) = self.lower_stmt(s, scope, &mut body) {
                            err = Some(e);
                            break 'cases;
                        }
                    }
                    hcases.push(HirSwitchCase { test, body });
                }
                if let Some(e) = err {
                    scope.pop_block();
                    return Err(e);
                }
                let (captured, disposables) = scope.pop_block();
                out.push(HirStmt::Switch { disc, cases: hcases });
                block_epilogue(out, captured, disposables);
            }
            // Labels are ignored (legacy `stmt.rs` does the same): `break`/
            // `continue` always target the innermost loop, and `Labeled` just
            // lowers its body.
            StmtKind::Break { .. } => out.push(HirStmt::Break),
            StmtKind::Continue { .. } => out.push(HirStmt::Continue),
            StmtKind::Labeled { body, .. } => self.lower_stmt(body, scope, out)?,
            StmtKind::Using {
                declarations,
                is_await,
            } => {
                for d in declarations {
                    let name = match &d.id {
                        Pattern::Identifier { name, .. } => name.clone(),
                        _ => return unsupported("destructuring using"),
                    };
                    let value = match &d.init {
                        Some(e) => self.lower_expr(e, scope)?,
                        None => HirExpr::Null,
                    };
                    let local = scope.alloc_local(name);
                    out.push(HirStmt::Let {
                        local,
                        value,
                        ty: HirType::Dynamic,
                    });
                    scope.record_disposable(local, *is_await);
                }
            }
            StmtKind::Throw { argument } => {
                let v = self.lower_expr(argument, scope)?;
                out.push(HirStmt::Throw(v));
            }
            StmtKind::Try {
                block,
                catch,
                finally,
            } => {
                let hblock = self.lower_block(block, scope)?;
                let hcatch = match catch {
                    Some(cc) => {
                        scope.push_block();
                        let param = match &cc.param {
                            None => None,
                            Some(Pattern::Identifier { name, .. }) => {
                                Some(scope.alloc_local(name.clone()))
                            }
                            Some(_) => {
                                scope.pop_block();
                                return unsupported("destructuring catch param");
                            }
                        };
                        let mut body = Vec::new();
                        let mut err = None;
                        match &cc.body.kind {
                            StmtKind::Block { stmts } => {
                                for s in stmts {
                                    if let Err(e) = self.lower_stmt(s, scope, &mut body) {
                                        err = Some(e);
                                        break;
                                    }
                                }
                            }
                            _ => {
                                if let Err(e) = self.lower_stmt(&cc.body, scope, &mut body) {
                                    err = Some(e);
                                }
                            }
                        }
                        if let Some(e) = err {
                            scope.pop_block();
                            return Err(e);
                        }
                        let (captured, disposables) = scope.pop_block();
                        block_epilogue(&mut body, captured, disposables);
                        Some(HirCatch { param, body })
                    }
                    None => None,
                };
                let hfinally = match finally {
                    Some(fin) => Some(self.lower_block(fin, scope)?),
                    None => None,
                };
                out.push(HirStmt::Try {
                    block: hblock,
                    catch: hcatch,
                    finally: hfinally,
                });
            }
            _ => return unsupported("statement kind"),
        }
        Ok(())
    }
}
