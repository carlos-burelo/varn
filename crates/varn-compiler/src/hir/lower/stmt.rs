use varn_core::ast::operators::AssignOp;
use varn_core::ast::{Decl, ExprKind, ForInit, Pattern, Stmt, StmtKind};
use varn_core::AnnKey;

use super::*;

impl<'a> Lowerer<'a> {
    /// Lower `for (name of start..end)` as a counted int loop.
    ///
    /// Emits the same shape a hand-written `for (let name = start; name < end;
    /// name = name + 1)` produces, with `end` hoisted into a temporary so it is
    /// evaluated exactly once — a range evaluates both bounds once, a
    /// `ForClassic` test does not.
    ///
    /// Bound evaluation order is start-then-end, matching the range expression
    /// this replaces. Ranges carry `step: 1` and no direction flag, so a
    /// descending `5..0` runs zero times under `<` exactly as it did through the
    /// iterator.
    #[allow(clippy::too_many_arguments)]
    fn lower_counted_range_loop(
        &mut self,
        name: Rc<str>,
        start: &varn_core::ast::Expr,
        end: &varn_core::ast::Expr,
        inclusive: bool,
        body: &Stmt,
        scope: &mut Scope,
        out: &mut Vec<HirStmt>,
    ) -> R<()> {
        scope.push_block();

        let start_val = match self.lower_expr(start, scope) {
            Ok(v) => v,
            Err(e) => {
                scope.pop_block();
                return Err(e);
            }
        };
        let loop_var = scope.alloc_local(name);
        out.push(HirStmt::Let {
            local: loop_var,
            value: start_val,
            ty: HirType::Int,
        });

        let end_val = match self.lower_expr(end, scope) {
            Ok(v) => v,
            Err(e) => {
                scope.pop_block();
                return Err(e);
            }
        };
        let limit = scope.alloc_temp();
        out.push(HirStmt::Let {
            local: limit,
            value: end_val,
            ty: HirType::Int,
        });

        let test = HirExpr::Binary {
            op: if inclusive {
                HirBinOp::Le
            } else {
                HirBinOp::Lt
            },
            lhs: Box::new(HirExpr::Var(HirBinding::Local(loop_var))),
            rhs: Box::new(HirExpr::Var(HirBinding::Local(limit))),
            ty: HirType::Int,
        };
        let update = vec![HirStmt::Assign {
            target: HirBinding::Local(loop_var),
            value: HirExpr::Binary {
                op: HirBinOp::Add,
                lhs: Box::new(HirExpr::Var(HirBinding::Local(loop_var))),
                rhs: Box::new(HirExpr::Int(1)),
                ty: HirType::Int,
            },
        }];

        let hbody = match self.lower_block(body, scope) {
            Ok(b) => b,
            Err(e) => {
                scope.pop_block();
                return Err(e);
            }
        };
        out.push(HirStmt::ForClassic {
            test,
            update,
            body: hbody,
        });

        let (captured, disposables) = scope.pop_block();
        block_epilogue(out, captured, disposables);
        Ok(())
    }

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

    fn lower_body_into(&mut self, body: &Stmt, scope: &mut Scope, out: &mut Vec<HirStmt>) -> R<()> {
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
                    let is_super = if let ExprKind::Member { object, .. } = &target.kind {
                        matches!(object.kind, ExprKind::Super)
                    } else {
                        false
                    };
                    if matches!(
                        op,
                        AssignOp::AndAssign | AssignOp::OrAssign | AssignOp::NullishAssign
                    ) || is_super
                    {
                        let expr = self.lower_expr(expression.as_ref(), scope)?;
                        out.push(HirStmt::Expr(expr));
                        return Ok(());
                    }

                    if let ExprKind::Member {
                        object,
                        property,
                        computed,
                        ..
                    } = &target.kind
                    {
                        let off = target.range.start.offset;

                        if let Some(mangled) = self.extension_set_members.get(&off).cloned() {
                            let recv = self.lower_expr(object.as_ref(), scope)?;
                            let val = self.lower_expr(value.as_ref(), scope)?;
                            out.push(HirStmt::Expr(self.ext_global_call(
                                mangled,
                                recv,
                                vec![val],
                            )));
                            return Ok(());
                        }
                        if !matches!(op, AssignOp::Assign) {
                            let bop = compound_to_bin(*op)?;
                            let ty = numeric_ty(self.ann, AnnKey::expr(target.id));
                            let object_hir = self.lower_expr(object.as_ref(), scope)?;
                            if *computed {
                                let index = self.lower_expr(property.as_ref(), scope)?;
                                let value = self.lower_expr(value.as_ref(), scope)?;
                                let is_arr = self.ann.get_array_index(AnnKey::expr(target.id));
                                let current_val = HirExpr::Index {
                                    object: Box::new(object_hir.clone()),
                                    index: Box::new(index.clone()),
                                    ty,
                                    is_array: is_arr,
                                };
                                let new_val = HirExpr::Binary {
                                    op: bop,
                                    lhs: Box::new(current_val),
                                    rhs: Box::new(value),
                                    ty,
                                };
                                out.push(HirStmt::SetIndex {
                                    object: object_hir,
                                    index,
                                    value: new_val,
                                    is_array: is_arr,
                                });
                            } else {
                                let name = match &property.kind {
                                    ExprKind::Identifier { name } => name.clone(),
                                    _ => {
                                        return Err(OptError::Unsupported(
                                            "hir: non-identifier property assign",
                                        ))
                                    }
                                };
                                let value = self.lower_expr(value.as_ref(), scope)?;
                                let current_val = HirExpr::Member {
                                    object: Box::new(object_hir.clone()),
                                    name: name.clone(),
                                    ty,
                                };
                                let new_val = HirExpr::Binary {
                                    op: bop,
                                    lhs: Box::new(current_val),
                                    rhs: Box::new(value),
                                    ty,
                                };
                                out.push(HirStmt::SetMember {
                                    object: object_hir,
                                    name,
                                    value: new_val,
                                });
                            }
                            return Ok(());
                        }
                        let object_hir = self.lower_expr(object.as_ref(), scope)?;
                        if *computed {
                            let index = self.lower_expr(property.as_ref(), scope)?;
                            let value = self.lower_expr(value.as_ref(), scope)?;
                            let is_arr = self.ann.get_array_index(AnnKey::expr(target.id));
                            out.push(HirStmt::SetIndex {
                                object: object_hir,
                                index,
                                value,
                                is_array: is_arr,
                            });
                        } else {
                            let name = match &property.kind {
                                ExprKind::Identifier { name } => name.clone(),
                                _ => {
                                    return Err(OptError::Unsupported(
                                        "hir: non-identifier property assign",
                                    ))
                                }
                            };
                            let value = self.lower_expr(value.as_ref(), scope)?;
                            if let Some(slot) =
                                self.ann.get_fixed_field_slot(AnnKey::expr(property.id))
                            {
                                out.push(HirStmt::SetFixedField {
                                    object: object_hir,
                                    slot,
                                    value,
                                });
                            } else {
                                out.push(HirStmt::SetMember {
                                    object: object_hir,
                                    name,
                                    value,
                                });
                            }
                        }
                        return Ok(());
                    }
                    let binding = match &target.kind {
                        ExprKind::Identifier { name } => self.resolve(name, scope),
                        _ => {
                            return Err(OptError::Unsupported("hir: non-identifier assign target"))
                        }
                    };
                    let val_expr = self.lower_expr(value.as_ref(), scope)?;
                    let value = match op {
                        AssignOp::Assign => val_expr,
                        _ => {
                            let bop = compound_to_bin(*op)?;
                            let ty = numeric_ty(self.ann, AnnKey::expr(target.id));
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
                    let e = self.lower_expr(expression.as_ref(), scope)?;
                    out.push(HirStmt::Expr(e));
                }
            }
            StmtKind::Decl(decl) => match decl.as_ref() {
                Decl::Variable(v) => {
                    for d in &v.declarators {
                        // A closure initializer may reference its own binding for
                        // recursion (`const fib = (n) => fib(n - 1)`). Allocate the
                        // slot before lowering the initializer so the closure
                        // captures its own local — matching how function
                        // declarations hoist their name. Non-closure initializers
                        // keep the original order, so an initializer that reads an
                        // outer binding of the same name is unaffected.
                        let is_closure_init = matches!(
                            d.init.as_ref().map(|e| &e.kind),
                            Some(ExprKind::Arrow { .. } | ExprKind::Function { .. })
                        );
                        if let (true, Pattern::Identifier { name, .. }) = (is_closure_init, &d.id) {
                            let local = scope.alloc_local(name.clone());
                            let value = self.lower_expr(d.init.as_ref().unwrap(), scope)?;
                            out.push(HirStmt::Let {
                                local,
                                value,
                                ty: HirType::Dynamic,
                            });
                        } else {
                            let value = match &d.init {
                                Some(e) => self.lower_expr(e, scope)?,
                                None => HirExpr::Null,
                            };
                            self.desugar_pattern_local(&d.id, value, scope, out)?;
                        }
                    }
                }

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
                    let cname = cl.id.clone().unwrap_or_else(|| Rc::from("anonymous"));
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
                Decl::SumType(st) => {
                    let hir_enum = self.lower_sum_type(st, scope)?;
                    let local = scope.alloc_local(st.id.clone());
                    out.push(HirStmt::Let {
                        local,
                        value: HirExpr::Enum(Box::new(hir_enum)),
                        ty: HirType::Dynamic,
                    });
                    for v in &st.variants {
                        let vlocal = scope.alloc_local(v.name.clone());
                        out.push(HirStmt::Let {
                            local: vlocal,
                            value: HirExpr::Member {
                                object: Box::new(HirExpr::Var(HirBinding::Local(local))),
                                name: v.name.clone(),
                                ty: HirType::Dynamic,
                            },
                            ty: HirType::Dynamic,
                        });
                    }
                }
                Decl::Interface(_) | Decl::TypeAlias(_) | Decl::Struct(_) => {}
                Decl::Namespace(ns) => {
                    self.lower_namespace(ns, scope, out)?;
                }
                _ => return Err(OptError::Unsupported("hir: nested declaration kind")),
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
                scope.push_block();
                if let Some(init) = init {
                    match init.as_ref() {
                        ForInit::Expr(e) => {
                            let e = self.lower_expr(e, scope)?;
                            out.push(HirStmt::Expr(e));
                        }
                        ForInit::Var { declarators, .. } => {
                            for d in declarators {
                                let value = match &d.init {
                                    Some(e) => self.lower_expr(e, scope)?,
                                    None => HirExpr::Null,
                                };
                                self.desugar_pattern_local(&d.id, value, scope, out)?;
                            }
                        }
                    }
                }
                let test = match test {
                    Some(t) => self.lower_expr(t, scope)?,
                    None => HirExpr::Bool(true),
                };

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
                // `for (x of a..b)` is a counted loop written with range syntax,
                // but it used to lower like any other iterable: allocate a Range
                // object, fetch its iterator, then per iteration fetch `next`,
                // call it, allocate a `{value, done}` result and read two
                // properties off it. Measured at 5M iterations that is 4.011 s
                // against 9.14 ms for the equivalent C-style loop — 438x.
                //
                // Ranges are always ascending with step 1 (`Heap::alloc_range`),
                // so the whole protocol collapses to an int counter.
                if let (
                    ExprKind::Range {
                        start,
                        end,
                        inclusive,
                    },
                    false,
                    Pattern::Identifier { name, .. },
                ) = (&right.kind, *is_await, left)
                {
                    return self.lower_counted_range_loop(
                        name.clone(),
                        start,
                        end,
                        *inclusive,
                        body,
                        scope,
                        out,
                    );
                }

                let iterable = self.lower_expr(right, scope)?;
                scope.push_block();
                let (loop_var, must_desugar) = match left {
                    Pattern::Identifier { name, .. } => (scope.alloc_local(name.clone()), false),
                    _ => (scope.alloc_temp(), true),
                };
                let mut hbody = Vec::new();
                if must_desugar {
                    self.desugar_pattern_local(
                        left,
                        HirExpr::Var(HirBinding::Local(loop_var)),
                        scope,
                        &mut hbody,
                    )?;
                }
                if let Err(e) = self.lower_body_into(body, scope, &mut hbody) {
                    scope.pop_block();
                    return Err(e);
                }
                let (captured, disposables) = scope.pop_block();
                block_epilogue(&mut hbody, captured, disposables);
                out.push(HirStmt::ForOf {
                    var: loop_var,
                    iterable,
                    body: hbody,
                    is_await: *is_await,
                });
            }
            StmtKind::ForIn {
                left, right, body, ..
            } => {
                let object = self.lower_expr(right, scope)?;
                scope.push_block();
                let (loop_var, must_desugar) = match left {
                    Pattern::Identifier { name, .. } => (scope.alloc_local(name.clone()), false),
                    _ => (scope.alloc_temp(), true),
                };
                let mut hbody = Vec::new();
                if must_desugar {
                    self.desugar_pattern_local(
                        left,
                        HirExpr::Var(HirBinding::Local(loop_var)),
                        scope,
                        &mut hbody,
                    )?;
                }
                if let Err(e) = self.lower_body_into(body, scope, &mut hbody) {
                    scope.pop_block();
                    return Err(e);
                }
                let (captured, disposables) = scope.pop_block();
                block_epilogue(&mut hbody, captured, disposables);
                out.push(HirStmt::ForIn {
                    var: loop_var,
                    object,
                    body: hbody,
                });
            }
            StmtKind::DoWhile { body, test } => {
                let hbody = self.lower_block(body, scope)?;
                let test = self.lower_expr(test, scope)?;
                out.push(HirStmt::DoWhile { body: hbody, test });
            }
            StmtKind::Switch {
                discriminant,
                cases,
            } => {
                let disc = self.lower_expr(discriminant, scope)?;

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
                out.push(HirStmt::Switch {
                    disc,
                    cases: hcases,
                });
                block_epilogue(out, captured, disposables);
            }

            StmtKind::Break { .. } => out.push(HirStmt::Break),
            StmtKind::Continue { .. } => out.push(HirStmt::Continue),
            StmtKind::Labeled { body, .. } => self.lower_stmt(body, scope, out)?,
            StmtKind::Using {
                declarations,
                is_await,
            } => {
                for d in declarations {
                    let value = match &d.init {
                        Some(e) => self.lower_expr(e, scope)?,
                        None => HirExpr::Null,
                    };
                    match &d.id {
                        Pattern::Identifier { name, .. } => {
                            let local = scope.alloc_local(name.clone());
                            out.push(HirStmt::Let {
                                local,
                                value,
                                ty: HirType::Dynamic,
                            });
                            scope.record_disposable(local, *is_await);
                        }
                        _ => {
                            let temp_local = scope.alloc_temp();
                            out.push(HirStmt::Let {
                                local: temp_local,
                                value,
                                ty: HirType::Dynamic,
                            });
                            scope.record_disposable(temp_local, *is_await);
                            self.desugar_pattern_local(
                                &d.id,
                                HirExpr::Var(HirBinding::Local(temp_local)),
                                scope,
                                out,
                            )?;
                        }
                    }
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
                            Some(_) => Some(scope.alloc_temp()),
                        };
                        let mut body = Vec::new();
                        if let Some(param_local) = param {
                            if let Some(pat) = &cc.param {
                                if !matches!(pat, Pattern::Identifier { .. }) {
                                    self.desugar_pattern_local(
                                        pat,
                                        HirExpr::Var(HirBinding::Local(param_local)),
                                        scope,
                                        &mut body,
                                    )?;
                                }
                            }
                        }
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
            _ => return Err(OptError::Unsupported("hir: statement kind")),
        }
        Ok(())
    }
}
