//! Expression AST→HIR lowering, plus `match` lowering and name resolution.

use std::rc::Rc;

use varn_core::ast::expr::{ArrayEl, ArrowBody, MatchBody, ObjectProp, PropKey, TemplatePart};
use varn_core::ast::operators::{AssignOp, UnaryOp};
use varn_core::ast::pattern::MatchPattern;
use varn_core::ast::{Arg, Expr, ExprKind, StmtKind};

use super::*;

impl<'a> Lowerer<'a> {
    /// Lower a `match` expression to a `HirExpr::Match`. Each case allocates its
    /// pattern bindings as locals in a per-case block scope, then lowers the arm
    /// body. Guards and record/sequence/type patterns fall back to legacy.
    fn lower_match(
        &mut self,
        subject: &Expr,
        cases: &[varn_core::ast::expr::MatchCase],
        scope: &mut Scope,
    ) -> R<HirExpr> {
        let subject = Box::new(self.lower_expr(subject, scope)?);
        let mut hcases = Vec::with_capacity(cases.len());
        for case in cases {
            if case.guard.is_some() {
                return unsupported("match guard");
            }
            scope.push_block();
            let test_res = self.lower_case_test(&case.pattern, scope);
            let test = match test_res {
                Ok(t) => t,
                Err(e) => {
                    scope.pop_block();
                    return Err(e);
                }
            };
            let mut body = Vec::new();
            let result = match &case.body {
                MatchBody::Block(s) => {
                    match &s.kind {
                        StmtKind::Block { stmts } => {
                            for st in stmts {
                                self.lower_stmt(st, scope, &mut body)?;
                            }
                        }
                        _ => self.lower_stmt(s, scope, &mut body)?,
                    }
                    None
                }
                MatchBody::Expr(e) => Some(self.lower_expr(e, scope)?),
            };
            let (captured, disposables) = scope.pop_block();
            block_epilogue(&mut body, captured, disposables);
            hcases.push(HirMatchCase { test, body, result });
        }
        Ok(HirExpr::Match {
            subject,
            cases: hcases,
        })
    }

    fn lower_case_test(&mut self, pat: &MatchPattern, scope: &mut Scope) -> R<HirCaseTest> {
        Ok(match pat {
            MatchPattern::Wildcard => HirCaseTest::Wildcard,
            MatchPattern::Literal(lit) => HirCaseTest::Literal(self.lower_expr(lit, scope)?),
            MatchPattern::Identifier(name) => HirCaseTest::Bind(scope.alloc_local(name.clone())),
            MatchPattern::EnumVariant {
                variant_name,
                bindings,
                ..
            } => {
                let mut binds = Vec::with_capacity(bindings.len());
                for b in bindings {
                    if &*b.name == "_" {
                        binds.push(None);
                    } else {
                        binds.push(Some(scope.alloc_local(b.name.clone())));
                    }
                }
                HirCaseTest::EnumVariant {
                    name: variant_name.clone(),
                    binds,
                }
            }
            _ => return unsupported("match pattern kind"),
        })
    }

    pub(super) fn lower_expr(&mut self, expr: &Expr, scope: &mut Scope) -> R<HirExpr> {
        let offset = expr.range.start.offset;
        match &expr.kind {
            ExprKind::IntLiteral { value, .. } => Ok(HirExpr::Int(*value)),
            ExprKind::FloatLiteral { value, .. } => Ok(HirExpr::Float(*value)),
            ExprKind::StrLiteral { value } => Ok(HirExpr::Str(Rc::from(value.as_str()))),
            ExprKind::BoolLiteral { value } => Ok(HirExpr::Bool(*value)),
            ExprKind::NullLiteral => Ok(HirExpr::Null),
            ExprKind::This => Ok(HirExpr::This),
            ExprKind::New { callee, args, .. } => {
                // `new C(args)` compiles to a plain Call; the VM constructs an
                // instance when the callee is a class (legacy `expr/mod.rs`).
                if let Some(mapping) = self.ann.get_call_mapping(offset) {
                    let identity = mapping.len() == args.len()
                        && mapping.iter().enumerate().all(|(i, m)| *m == Some(i));
                    if !identity {
                        return unsupported("new with non-trivial arg mapping");
                    }
                }
                let mut hargs = Vec::with_capacity(args.len());
                for a in args {
                    match a {
                        Arg::Positional(e) => hargs.push(self.lower_expr(e, scope)?),
                        _ => return unsupported("new with named/spread arg"),
                    }
                }
                let callee = Box::new(self.lower_expr(callee, scope)?);
                Ok(HirExpr::Call {
                    callee,
                    args: hargs,
                    ty: HirType::Dynamic,
                })
            }
            ExprKind::Paren { expression } => self.lower_expr(expression, scope),
            ExprKind::Identifier { name } => Ok(HirExpr::Var(self.resolve(name, scope))),
            ExprKind::Binary { op, left, right } => {
                let lhs = Box::new(self.lower_expr(left, scope)?);
                let rhs = Box::new(self.lower_expr(right, scope)?);
                let ty = numeric_ty(self.ann, offset);
                Ok(HirExpr::Binary {
                    op: bin_op(*op)?,
                    lhs,
                    rhs,
                    ty,
                })
            }
            ExprKind::Unary { op, operand, .. } => {
                // Unary `+` is a transparent no-op.
                if matches!(op, UnaryOp::Plus) {
                    return self.lower_expr(operand, scope);
                }
                let operand = Box::new(self.lower_expr(operand, scope)?);
                Ok(HirExpr::Unary {
                    op: un_op(*op)?,
                    operand,
                    ty: HirType::Dynamic,
                })
            }
            ExprKind::Call {
                callee,
                args,
                optional,
                ..
            } => {
                if *optional {
                    return unsupported("optional call");
                }
                // Intrinsics, extension calls, and calls whose arguments need
                // reordering/default-filling (`get_call_mapping`) are desugared
                // by the legacy codegen → fall back until those land.
                if self.ann.get_intrinsic(offset).is_some() {
                    return unsupported("intrinsic call");
                }
                // A non-trivial call mapping reorders args or fills defaults —
                // replicating that is §1.10. An *identity* mapping (param i ← arg
                // i, no gaps) is just plain positional, so let those through.
                if let Some(mapping) = self.ann.get_call_mapping(offset) {
                    let identity = mapping.len() == args.len()
                        && mapping.iter().enumerate().all(|(i, m)| *m == Some(i));
                    if !identity {
                        return unsupported("call with non-trivial arg mapping");
                    }
                }
                if self.extension_calls.contains_key(&offset) {
                    return unsupported("extension call");
                }
                // Only simple positional calls (no named/spread/defaults).
                let mut hargs = Vec::with_capacity(args.len());
                for a in args {
                    match a {
                        Arg::Positional(e) => hargs.push(self.lower_expr(e, scope)?),
                        _ => return unsupported("named/spread arg"),
                    }
                }
                // Method call: callee is a non-computed, non-optional `.name`
                // member (and not a `super.` call) → `CallMethod` with an IC.
                if let ExprKind::Member {
                    object,
                    property,
                    computed: false,
                    optional: false,
                } = &callee.kind
                {
                    if !matches!(object.kind, ExprKind::Super) {
                        if let ExprKind::Identifier { name } = &property.kind {
                            let recv = Box::new(self.lower_expr(object, scope)?);
                            return Ok(HirExpr::MethodCall {
                                recv,
                                name: name.clone(),
                                args: hargs,
                                ty: HirType::Dynamic,
                            });
                        }
                    }
                }
                // Statically-resolved self-recursion → `CallSelf` (see HIR doc).
                if self.is_self_call(callee, scope) {
                    return Ok(HirExpr::SelfCall {
                        args: hargs,
                        ty: HirType::Dynamic,
                    });
                }
                let callee = Box::new(self.lower_expr(callee, scope)?);
                Ok(HirExpr::Call {
                    callee,
                    args: hargs,
                    ty: HirType::Dynamic,
                })
            }
            ExprKind::Member {
                object,
                property,
                computed,
                optional,
            } => {
                if *optional {
                    // Optional chaining needs IsNull + jump; deferred.
                    return unsupported("optional member access");
                }
                if *computed {
                    // `object[index]` → GetIndex.
                    let object = Box::new(self.lower_expr(object, scope)?);
                    let index = Box::new(self.lower_expr(property, scope)?);
                    return Ok(HirExpr::Index {
                        object,
                        index,
                        ty: HirType::Dynamic,
                    });
                }
                // Non-computed `object.name` property read. Module-slot reads
                // (`LoadModuleSlot`) and extension members are desugared
                // differently by the legacy codegen → fall back for now.
                if self.ann.get_slot_idx(offset).is_some() {
                    return unsupported("module-slot member access");
                }
                if self.extension_members.contains_key(&offset) {
                    return unsupported("extension member access");
                }
                if matches!(object.kind, ExprKind::Super) {
                    return unsupported("super member access");
                }
                let name = match &property.kind {
                    ExprKind::Identifier { name } => name.clone(),
                    _ => return unsupported("non-identifier property"),
                };
                let object = Box::new(self.lower_expr(object, scope)?);
                Ok(HirExpr::Member {
                    object,
                    name,
                    ty: HirType::Dynamic,
                })
            }
            ExprKind::Logical { op, left, right } => {
                let lhs = Box::new(self.lower_expr(left, scope)?);
                let rhs = Box::new(self.lower_expr(right, scope)?);
                Ok(HirExpr::Logical {
                    op: logical_op(*op),
                    lhs,
                    rhs,
                })
            }
            ExprKind::Conditional {
                test,
                consequent,
                alternate,
            } => {
                let test = Box::new(self.lower_expr(test, scope)?);
                let cons = Box::new(self.lower_expr(consequent, scope)?);
                let alt = Box::new(self.lower_expr(alternate, scope)?);
                Ok(HirExpr::Conditional { test, cons, alt })
            }
            ExprKind::Update {
                op,
                prefix,
                operand,
            } => {
                // Member/index update targets are §1.4; identifiers only here.
                let ExprKind::Identifier { name } = &operand.kind else {
                    return unsupported("update on non-identifier target");
                };
                let target = self.resolve(name, scope);
                Ok(HirExpr::Update {
                    target,
                    op: update_op(*op),
                    prefix: *prefix,
                })
            }
            ExprKind::Array { elements } => {
                // Simple array literal: plain element exprs, no spread/holes.
                let mut out = Vec::with_capacity(elements.len());
                for el in elements {
                    match el {
                        ArrayEl::Expr(e) => out.push(self.lower_expr(e, scope)?),
                        ArrayEl::Spread(_) => return unsupported("array spread element"),
                        ArrayEl::Hole => return unsupported("array hole"),
                    }
                }
                Ok(HirExpr::Array(out))
            }
            ExprKind::Object { properties } => {
                // Fixed-shape object: all-static keys, plain value props only.
                let mut keys = Vec::with_capacity(properties.len());
                let mut values = Vec::with_capacity(properties.len());
                for prop in properties {
                    match prop {
                        ObjectProp::Property {
                            key,
                            value,
                            computed: false,
                            ..
                        } => {
                            let k: Rc<str> = match key {
                                PropKey::Identifier(s) | PropKey::Str(s) => Rc::from(s.as_str()),
                                PropKey::Int(n) => Rc::from(n.to_string().as_str()),
                                PropKey::Computed(_) => return unsupported("computed object key"),
                            };
                            keys.push(k);
                            values.push(self.lower_expr(value, scope)?);
                        }
                        _ => return unsupported("object method/getter/setter/spread/computed"),
                    }
                }
                // Empty `{}` uses the legacy BuildObject path; defer.
                if keys.is_empty() {
                    return unsupported("empty object literal");
                }
                Ok(HirExpr::Object { keys, values })
            }
            ExprKind::Function {
                fn_id,
                params,
                body,
                is_async,
                is_generator,
                ..
            } => {
                // A named function expression can reference itself; resolving
                // that correctly needs a self-binding we don't model yet.
                if fn_id.is_some() {
                    return unsupported("named function expression");
                }
                let (func, upvalues) = self.lower_function_like(
                    Rc::from("<anon>"),
                    params,
                    *is_async,
                    *is_generator,
                    false,
                    false,
                    BodyRef::Block(body),
                    &[],
                    scope,
                )?;
                Ok(HirExpr::Closure {
                    func: Box::new(func),
                    upvalues,
                })
            }
            ExprKind::Arrow {
                params,
                body,
                is_async,
                ..
            } => {
                let body_ref = match body.as_ref() {
                    ArrowBody::Expr(e) => BodyRef::ExprBody(e),
                    ArrowBody::Block(s) => BodyRef::Block(s),
                };
                let (func, upvalues) = self.lower_function_like(
                    Rc::from("<arrow>"),
                    params,
                    *is_async,
                    false,
                    false,
                    false,
                    body_ref,
                    &[],
                    scope,
                )?;
                Ok(HirExpr::Closure {
                    func: Box::new(func),
                    upvalues,
                })
            }
            ExprKind::Match { subject, cases } => self.lower_match(subject, cases, scope),
            ExprKind::CharLiteral { value } => Ok(HirExpr::Char(*value)),
            // (Remaining unsupported exprs that still fall back: DecimalLiteral/
            // BigIntLiteral — need `rust_decimal`; `Pipeline`, `Is`, `Await`.)
            // `as` / `satisfies` are type-only — transparent at codegen.
            ExprKind::As { expression, .. } | ExprKind::Satisfies { expression, .. } => {
                self.lower_expr(expression, scope)
            }
            ExprKind::NonNull { expression } => {
                Ok(HirExpr::NonNull(Box::new(self.lower_expr(expression, scope)?)))
            }
            ExprKind::Sequence { expressions } => {
                let mut out = Vec::with_capacity(expressions.len());
                for e in expressions {
                    out.push(self.lower_expr(e, scope)?);
                }
                Ok(HirExpr::Sequence(out))
            }
            ExprKind::Range {
                start,
                end,
                inclusive,
            } => Ok(HirExpr::Range {
                start: Box::new(self.lower_expr(start, scope)?),
                end: Box::new(self.lower_expr(end, scope)?),
                inclusive: *inclusive,
            }),
            ExprKind::Template { parts } => {
                let mut out = Vec::with_capacity(parts.len());
                for p in parts {
                    match p {
                        TemplatePart::Literal(s) => {
                            out.push(HirTemplatePart::Str(Rc::from(s.as_str())))
                        }
                        TemplatePart::Interpolation(e) => {
                            out.push(HirTemplatePart::Expr(self.lower_expr(e, scope)?))
                        }
                    }
                }
                Ok(HirExpr::Template(out))
            }
            // Assignment in expression position (statement-level assigns are
            // handled in `lower_stmt`). Yields the assigned value.
            ExprKind::Assign { op, target, value } => {
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
                    let tgt = if *computed {
                        let index = self.lower_expr(property, scope)?;
                        HirAssignTarget::Index {
                            object: object_hir,
                            index,
                        }
                    } else {
                        let name = match &property.kind {
                            ExprKind::Identifier { name } => name.clone(),
                            _ => return unsupported("non-identifier property assign"),
                        };
                        HirAssignTarget::Member {
                            object: object_hir,
                            name,
                        }
                    };
                    let v = self.lower_expr(value, scope)?;
                    return Ok(HirExpr::Assign {
                        target: Box::new(tgt),
                        value: Box::new(v),
                    });
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
                        let ty = numeric_ty(self.ann, offset);
                        HirExpr::Binary {
                            op: bop,
                            lhs: Box::new(HirExpr::Var(binding.clone())),
                            rhs: Box::new(val_expr),
                            ty,
                        }
                    }
                };
                Ok(HirExpr::Assign {
                    target: Box::new(HirAssignTarget::Var(binding)),
                    value: Box::new(value),
                })
            }
            _ => unsupported("expression kind"),
        }
    }

    /// Whether `callee` is a statically-guaranteed reference to the enclosing
    /// function: a bare identifier equal to the current function's name, not
    /// shadowed by a local/param, and never reassigned in the module. Mirrors
    /// legacy `can_emit_self_call` (async/generator/rest/`this` cases are
    /// already excluded upstream because such functions fall back to legacy).
    fn is_self_call(&self, callee: &Expr, scope: &Scope) -> bool {
        let ExprKind::Identifier { name } = &callee.kind else {
            return false;
        };
        match &self.current_fn {
            Some(cur) if cur == name => {}
            _ => return false,
        }
        // Self-recursion is `CallSelf` (direct, no closure lookup) when the name
        // is not shadowed by a param/local of *this* function and is not
        // reassigned. Checking only the current frame — not capturing from a
        // parent — matches legacy `name_resolves_locally`, so a nested function
        // recurses via `CallSelf` instead of an upvalue to its own slot (which
        // would be a use-before-def the register allocator can't model).
        scope.resolve_in_current_frame(name).is_none() && !self.ann.is_reassigned_name(name)
    }

    pub(super) fn resolve(&self, name: &Rc<str>, scope: &mut Scope) -> HirBinding {
        if let Some(b) = scope.resolve(name) {
            b
        } else {
            // Unresolved locally -> module global (covers builtins like `print`
            // too, which the VM resolves by name).
            HirBinding::Global(name.clone())
        }
    }
}
