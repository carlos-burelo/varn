use rust_decimal::Decimal;
use std::rc::Rc;
use std::str::FromStr;

use varn_core::ast::expr::{ArrayEl, ArrowBody, MatchBody, ObjectProp, PropKey, TemplatePart};
use varn_core::ast::operators::{AssignOp, UnaryOp};
use varn_core::ast::pattern::MatchPattern;
use varn_core::ast::{Arg, Expr, ExprKind, StmtKind};

use super::*;

impl<'a> Lowerer<'a> {
    fn lower_match(
        &mut self,
        subject: &Expr,
        cases: &[varn_core::ast::expr::MatchCase],
        scope: &mut Scope,
    ) -> R<HirExpr> {
        let subject = Box::new(self.lower_expr(subject, scope)?);
        let mut hcases = Vec::with_capacity(cases.len());
        for case in cases {
            scope.push_block();
            let test_res = self.lower_case_test(&case.pattern, scope);
            let test = match test_res {
                Ok(t) => t,
                Err(e) => {
                    scope.pop_block();
                    return Err(e);
                }
            };

            let guard = match &case.guard {
                Some(g) => Some(self.lower_expr(g, scope)?),
                None => None,
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
            hcases.push(HirMatchCase {
                test,
                guard,
                body,
                result,
            });
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
            MatchPattern::Record { fields, .. } => {
                let mut binds = Vec::with_capacity(fields.len());
                for (field_name, sub_pat) in fields {
                    let binding = match sub_pat {
                        Some(MatchPattern::Identifier(n)) => n.clone(),
                        _ => field_name.clone(),
                    };
                    if &*binding == "_" {
                        binds.push((field_name.clone(), None));
                    } else {
                        binds.push((field_name.clone(), Some(scope.alloc_local(binding))));
                    }
                }
                HirCaseTest::Record { fields: binds }
            }
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

            MatchPattern::Sequence(_) | MatchPattern::Type { .. } => HirCaseTest::Wildcard,
        })
    }

    fn lower_assign_target(&mut self, expr: &Expr, scope: &mut Scope) -> R<HirAssignTarget> {
        match &expr.kind {
            ExprKind::Identifier { name } => Ok(HirAssignTarget::Var(self.resolve(name, scope))),
            ExprKind::Member {
                object,
                property,
                computed,
                ..
            } => {
                if matches!(object.kind, ExprKind::Super) {
                    if *computed {
                        let index = self.lower_expr(property, scope)?;
                        Ok(HirAssignTarget::SuperIndex { index })
                    } else {
                        let name = match &property.kind {
                            ExprKind::Identifier { name } => name.clone(),
                            _ => {
                                return Err(OptError::Unsupported(
                                    "hir: non-identifier super property assign",
                                ))
                            }
                        };
                        Ok(HirAssignTarget::SuperMember { name })
                    }
                } else {
                    let object_hir = self.lower_expr(object, scope)?;
                    if *computed {
                        let index = self.lower_expr(property, scope)?;
                        let is_array = self.ann.get_array_index(expr.range.start.offset);
                        Ok(HirAssignTarget::Index {
                            object: object_hir,
                            index,
                            is_array,
                        })
                    } else {
                        if let Some(slot) =
                            self.ann.get_fixed_field_slot(property.range.start.offset)
                        {
                            return Ok(HirAssignTarget::SetFixedField {
                                object: object_hir,
                                slot,
                            });
                        }
                        let name = match &property.kind {
                            ExprKind::Identifier { name } => name.clone(),
                            _ => {
                                return Err(OptError::Unsupported(
                                    "hir: non-identifier property assign",
                                ))
                            }
                        };
                        Ok(HirAssignTarget::Member {
                            object: object_hir,
                            name,
                        })
                    }
                }
            }
            _ => return Err(OptError::Unsupported("hir: non-identifier assign target")),
        }
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
                let hargs = self.lower_call_args(args, offset, scope)?;
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
                    let hargs = self.lower_call_args(args, offset, scope)?;
                    let callee_hir = self.lower_expr(callee, scope)?;
                    return Ok(HirExpr::OptionalChain {
                        object: Box::new(callee_hir),
                        property: HirOptionalProperty::Call(hargs),
                    });
                }
                // Free-function intrinsic (`abs(x)` from `std:math`): lower to
                // IntrinsicCall with a synthetic null receiver so it reuses the
                // method-form codegen (VM reads args[1]; JIT inlines
                // fabs/sqrtsd/roundsd). Keyed at the callee identifier offset.
                if let ExprKind::Identifier { .. } = &callee.kind {
                    if let Some(wire_byte) =
                        self.ann.get_intrinsic(callee.range.start.offset)
                    {
                        let has_spread = args.iter().any(|a| matches!(a, Arg::Spread(_)));
                        if !has_spread {
                            let hargs = self.lower_call_args(args, offset, scope)?;
                            return Ok(HirExpr::IntrinsicCall {
                                object: Box::new(HirExpr::Null),
                                args: hargs,
                                wire_byte,
                                ty: HirType::Dynamic,
                            });
                        }
                    }
                }

                // Intrinsic / native-op annotations are keyed by the METHOD
                // NAME offset: chained calls (`a.f().g()`) share their
                // expression start offset, so keying by expr start let an
                // outer call's annotation leak into the inner call.
                let method_key = match &callee.kind {
                    ExprKind::Member {
                        property,
                        computed: false,
                        ..
                    } => Some(property.range.start.offset),
                    _ => None,
                };
                if let Some(wire_byte) = method_key.and_then(|o| self.ann.get_intrinsic(o)) {
                    if let ExprKind::Member {
                        object,
                        computed: false,
                        ..
                    } = &callee.kind
                    {
                        let has_spread = args.iter().any(|a| matches!(a, Arg::Spread(_)));
                        if !has_spread {
                            let hobj = self.lower_expr(object, scope)?;
                            let hargs = self.lower_call_args(args, offset, scope)?;
                            return Ok(HirExpr::IntrinsicCall {
                                object: Box::new(hobj),
                                args: hargs,
                                wire_byte,
                                ty: HirType::Dynamic,
                            });
                        }
                    }
                }
                if let Some(op_id) = method_key.and_then(|o| self.ann.get_native_op(o)) {
                    if let ExprKind::Member {
                        object,
                        computed: false,
                        ..
                    } = &callee.kind
                    {
                        let has_spread = args.iter().any(|a| matches!(a, Arg::Spread(_)));
                        if !has_spread {
                            let hobj = self.lower_expr(object, scope)?;
                            let hargs = self.lower_call_args(args, offset, scope)?;
                            return Ok(HirExpr::NativeMethodCall {
                                object: Box::new(hobj),
                                args: hargs,
                                op_id,
                                ty: HirType::Dynamic,
                            });
                        }
                    }
                }

                if let Some(mangled) = self.extension_calls.get(&offset).cloned() {
                    if let ExprKind::Member {
                        object,
                        optional: mem_opt,
                        ..
                    } = &callee.kind
                    {
                        let recv = self.lower_expr(object, scope)?;
                        let call_args = self.lower_call_args(args, offset, scope)?;
                        if *mem_opt {
                            return Ok(HirExpr::OptionalChain {
                                object: Box::new(recv),
                                property: HirOptionalProperty::ExtensionCall(mangled, call_args),
                            });
                        }
                        return Ok(self.ext_global_call(mangled, recv, call_args));
                    }
                }

                let hargs = self.lower_call_args(args, offset, scope)?;

                if matches!(callee.kind, ExprKind::Super) {
                    return Ok(HirExpr::SuperCall { args: hargs });
                }

                if let ExprKind::Member {
                    object,
                    property,
                    computed: false,
                    optional: false,
                } = &callee.kind
                {
                    if matches!(object.kind, ExprKind::Super) {
                        if let ExprKind::Identifier { name } = &property.kind {
                            return Ok(HirExpr::SuperMethodCall {
                                name: name.clone(),
                                args: hargs,
                            });
                        }
                    }
                }

                if let ExprKind::Member {
                    object,
                    property,
                    computed: false,
                    optional: mem_opt,
                } = &callee.kind
                {
                    if !matches!(object.kind, ExprKind::Super) {
                        if let ExprKind::Identifier { name } = &property.kind {
                            let recv = Box::new(self.lower_expr(object, scope)?);
                            if *mem_opt {
                                return Ok(HirExpr::OptionalChain {
                                    object: recv,
                                    property: HirOptionalProperty::MethodCall(name.clone(), hargs),
                                });
                            }
                            let ty = self.value_ty(property.range.start.offset);
                            return Ok(HirExpr::MethodCall {
                                recv,
                                name: name.clone(),
                                args: hargs,
                                ty,
                            });
                        }
                    }
                }

                let has_spread = args.iter().any(|a| matches!(a, Arg::Spread(_)));
                if !has_spread && self.is_self_call(callee, scope) {
                    return Ok(HirExpr::SelfCall {
                        args: hargs,
                        ty: HirType::Dynamic,
                    });
                }
                let call_ty = self.value_ty(offset);
                let callee = Box::new(self.lower_expr(callee, scope)?);
                Ok(HirExpr::Call {
                    callee,
                    args: hargs,
                    ty: call_ty,
                })
            }
            ExprKind::Member {
                object,
                property,
                computed,
                optional,
            } => {
                if *optional {
                    let object_hir = self.lower_expr(object, scope)?;
                    let opt_prop = if *computed {
                        let idx = self.lower_expr(property, scope)?;
                        HirOptionalProperty::Index(Box::new(idx))
                    } else {
                        let name = match &property.kind {
                            ExprKind::Identifier { name } => name.clone(),
                            _ => return Err(OptError::Unsupported("hir: non-identifier property")),
                        };
                        if self.ann.get_slot_idx(offset).is_some() {
                            let slot_idx = self.ann.get_slot_idx(offset).unwrap() as u16;
                            HirOptionalProperty::ModuleSlot(slot_idx)
                        } else if let Some(mangled) = self.extension_members.get(&offset).cloned() {
                            HirOptionalProperty::Extension(mangled)
                        } else {
                            HirOptionalProperty::Member(name)
                        }
                    };
                    return Ok(HirExpr::OptionalChain {
                        object: Box::new(object_hir),
                        property: opt_prop,
                    });
                }
                if *computed {
                    let ty = self.value_ty(property.range.start.offset);
                    let object = Box::new(self.lower_expr(object, scope)?);
                    let index = Box::new(self.lower_expr(property, scope)?);
                    let is_array = self.ann.get_array_index(offset);
                    return Ok(HirExpr::Index {
                        object,
                        index,
                        ty,
                        is_array,
                    });
                }

                if let Some(slot_idx) = self.ann.get_slot_idx(offset) {
                    let object_hir = self.lower_expr(object, scope)?;
                    return Ok(HirExpr::ModuleSlot {
                        object: Box::new(object_hir),
                        slot: slot_idx as u16,
                        ty: HirType::Dynamic,
                    });
                }

                if let Some(slot) = self.ann.get_fixed_field_slot(property.range.start.offset) {
                    let ty = self.value_ty(property.range.start.offset);
                    let object_hir = self.lower_expr(object, scope)?;
                    return Ok(HirExpr::GetFixedField {
                        object: Box::new(object_hir),
                        slot,
                        ty,
                    });
                }

                if let Some(mangled) = self.extension_members.get(&offset).cloned() {
                    let recv = self.lower_expr(object, scope)?;
                    return Ok(self.ext_global_call(mangled, recv, vec![]));
                }
                if matches!(object.kind, ExprKind::Super) {
                    let name = match &property.kind {
                        ExprKind::Identifier { name } => name.clone(),
                        _ => {
                            return Err(OptError::Unsupported("hir: non-identifier super property"))
                        }
                    };
                    return Ok(HirExpr::SuperMember { name });
                }
                let name = match &property.kind {
                    ExprKind::Identifier { name } => name.clone(),
                    _ => return Err(OptError::Unsupported("hir: non-identifier property")),
                };
                let ty = self.value_ty(property.range.start.offset);
                let object = Box::new(self.lower_expr(object, scope)?);
                Ok(HirExpr::Member { object, name, ty })
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
                let target = self.lower_assign_target(operand, scope)?;
                Ok(HirExpr::Update {
                    target: Box::new(target),
                    op: update_op(*op),
                    prefix: *prefix,
                })
            }
            ExprKind::Array { elements } => {
                let mut out = Vec::with_capacity(elements.len());
                for el in elements {
                    match el {
                        ArrayEl::Expr(e) => out.push(HirArrayEl::Expr(self.lower_expr(e, scope)?)),
                        ArrayEl::Spread(e) => {
                            out.push(HirArrayEl::Spread(self.lower_expr(e, scope)?))
                        }
                        ArrayEl::Hole => out.push(HirArrayEl::Hole),
                    }
                }
                Ok(HirExpr::Array(out))
            }
            ExprKind::Object { properties } => {
                let mut props = Vec::with_capacity(properties.len());
                for prop in properties {
                    match prop {
                        ObjectProp::Property { key, value, .. } => {
                            let k = match key {
                                PropKey::Computed(e) => {
                                    HirPropKey::Computed(self.lower_expr(e, scope)?)
                                }
                                PropKey::Identifier(s) | PropKey::Str(s) => {
                                    HirPropKey::Static(Rc::from(s.as_str()))
                                }
                                PropKey::Int(n) => {
                                    HirPropKey::Static(Rc::from(n.to_string().as_str()))
                                }
                            };
                            let val = self.lower_expr(value, scope)?;
                            props.push(HirObjectProp::Property { key: k, value: val });
                        }
                        ObjectProp::Method {
                            key,
                            params,
                            body,
                            is_async,
                            is_generator,
                            ..
                        } => {
                            let k = match key {
                                PropKey::Computed(e) => {
                                    HirPropKey::Computed(self.lower_expr(e, scope)?)
                                }
                                PropKey::Identifier(s) | PropKey::Str(s) => {
                                    HirPropKey::Static(Rc::from(s.as_str()))
                                }
                                PropKey::Int(n) => {
                                    HirPropKey::Static(Rc::from(n.to_string().as_str()))
                                }
                            };
                            let method_name = match key {
                                PropKey::Identifier(s) | PropKey::Str(s) => s.clone(),
                                PropKey::Int(n) => n.to_string(),
                                PropKey::Computed(_) => "<computed>".to_owned(),
                            };
                            let (func, upvalues) = self.lower_function_like(
                                Rc::from(method_name.as_str()),
                                params,
                                *is_async,
                                *is_generator,
                                false,
                                true,
                                BodyRef::Block(body),
                                &[],
                                scope,
                            )?;
                            props.push(HirObjectProp::Method {
                                key: k,
                                func,
                                upvalues,
                            });
                        }
                        ObjectProp::Spread { argument, .. } => {
                            let arg = self.lower_expr(argument, scope)?;
                            props.push(HirObjectProp::Spread(arg));
                        }
                        _ => {}
                    }
                }
                Ok(HirExpr::Object { properties: props })
            }
            ExprKind::Function {
                fn_id,
                params,
                body,
                is_async,
                is_generator,
                ..
            } => {
                let name = fn_id.clone().unwrap_or_else(|| Rc::from("<anon>"));
                let (func, upvalues) = self.lower_function_like(
                    name,
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
            ExprKind::DecimalLiteral { raw } => {
                let d = Decimal::from_str(raw.trim_end_matches('d')).unwrap_or(Decimal::ZERO);
                Ok(HirExpr::Decimal(d))
            }
            ExprKind::BigIntLiteral { raw } => {
                let s = raw.trim_end_matches('n').replace('_', "");
                let parsed = if let Some(r) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X"))
                {
                    i128::from_str_radix(r, 16)
                } else if let Some(r) = s.strip_prefix("0o").or_else(|| s.strip_prefix("0O")) {
                    i128::from_str_radix(r, 8)
                } else if let Some(r) = s.strip_prefix("0b").or_else(|| s.strip_prefix("0B")) {
                    i128::from_str_radix(r, 2)
                } else {
                    s.parse()
                };

                let num = parsed.unwrap_or(0);
                Ok(HirExpr::BigInt(num))
            }
            ExprKind::RegexLiteral { pattern, flags } => Ok(HirExpr::Regex {
                pattern: Rc::from(pattern.as_str()),
                flags: Rc::from(flags.as_str()),
            }),
            ExprKind::Await { argument } => {
                let inner = self.lower_expr(argument, scope)?;
                Ok(HirExpr::Await(Box::new(inner)))
            }
            ExprKind::Spawn { argument } => {
                let inner = self.lower_expr(argument, scope)?;
                Ok(HirExpr::Spawn(Box::new(inner)))
            }
            ExprKind::Yield { argument, .. } => {
                let inner = if let Some(arg) = argument {
                    self.lower_expr(arg, scope)?
                } else {
                    HirExpr::Null
                };
                Ok(HirExpr::Yield(Box::new(inner)))
            }

            ExprKind::As { expression, .. } | ExprKind::Satisfies { expression, .. } => {
                self.lower_expr(expression, scope)
            }
            ExprKind::NonNull { expression } => Ok(HirExpr::NonNull(Box::new(
                self.lower_expr(expression, scope)?,
            ))),
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

            ExprKind::Assign { op, target, value } => {
                if let ExprKind::Member {
                    object,
                    property,
                    computed,
                    ..
                } = &target.kind
                {
                    let off = target.range.start.offset;

                    if let Some(mangled) = self.extension_set_members.get(&off).cloned() {
                        let recv = self.lower_expr(object, scope)?;
                        let val = self.lower_expr(value, scope)?;
                        return Ok(self.ext_global_call(mangled, recv, vec![val]));
                    }
                    if matches!(object.kind, ExprKind::Super) {
                        if *computed {
                            let index = self.lower_expr(property, scope)?;
                            let value = self.lower_expr(value, scope)?;
                            if matches!(op, AssignOp::Assign) {
                                let tgt = HirAssignTarget::SuperIndex { index };
                                return Ok(HirExpr::Assign {
                                    target: Box::new(tgt),
                                    value: Box::new(value),
                                });
                            } else {
                                let bop = compound_to_bin(*op)?;
                                let ty = numeric_ty(self.ann, target.range.start.offset);
                                let current_val = HirExpr::Index {
                                    object: Box::new(HirExpr::Super),
                                    index: Box::new(index.clone()),
                                    ty: HirType::Dynamic,
                                    is_array: false,
                                };
                                let new_val = HirExpr::Binary {
                                    op: bop,
                                    lhs: Box::new(current_val),
                                    rhs: Box::new(value),
                                    ty,
                                };
                                let tgt = HirAssignTarget::SuperIndex { index };
                                return Ok(HirExpr::Assign {
                                    target: Box::new(tgt),
                                    value: Box::new(new_val),
                                });
                            }
                        } else {
                            let name = match &property.kind {
                                ExprKind::Identifier { name } => name.clone(),
                                _ => {
                                    return Err(OptError::Unsupported(
                                        "hir: non-identifier super property assign",
                                    ))
                                }
                            };
                            let value = self.lower_expr(value, scope)?;
                            if matches!(op, AssignOp::Assign) {
                                let tgt = HirAssignTarget::SuperMember { name };
                                return Ok(HirExpr::Assign {
                                    target: Box::new(tgt),
                                    value: Box::new(value),
                                });
                            } else {
                                let bop = compound_to_bin(*op)?;
                                let ty = numeric_ty(self.ann, target.range.start.offset);
                                let current_val = HirExpr::SuperMember { name: name.clone() };
                                let new_val = HirExpr::Binary {
                                    op: bop,
                                    lhs: Box::new(current_val),
                                    rhs: Box::new(value),
                                    ty,
                                };
                                let tgt = HirAssignTarget::SuperMember { name };
                                return Ok(HirExpr::Assign {
                                    target: Box::new(tgt),
                                    value: Box::new(new_val),
                                });
                            }
                        }
                    }
                    if matches!(
                        op,
                        AssignOp::AndAssign | AssignOp::OrAssign | AssignOp::NullishAssign
                    ) {
                        let lop = match op {
                            AssignOp::AndAssign => HirLogicalOp::And,
                            AssignOp::OrAssign => HirLogicalOp::Or,
                            AssignOp::NullishAssign => HirLogicalOp::Nullish,
                            _ => unreachable!(),
                        };
                        let ty = numeric_ty(self.ann, target.range.start.offset);
                        let object_hir = self.lower_expr(object, scope)?;
                        if *computed {
                            let index = self.lower_expr(property, scope)?;
                            let value = self.lower_expr(value, scope)?;
                            let is_arr = self.ann.get_array_index(target.range.start.offset);
                            let current_val = HirExpr::Index {
                                object: Box::new(object_hir.clone()),
                                index: Box::new(index.clone()),
                                ty,
                                is_array: is_arr,
                            };
                            let tgt = HirAssignTarget::Index {
                                object: object_hir,
                                index,
                                is_array: is_arr,
                            };
                            let assign = HirExpr::Assign {
                                target: Box::new(tgt),
                                value: Box::new(value),
                            };
                            return Ok(HirExpr::Logical {
                                op: lop,
                                lhs: Box::new(current_val),
                                rhs: Box::new(assign),
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
                            let value = self.lower_expr(value, scope)?;
                            let current_val = HirExpr::Member {
                                object: Box::new(object_hir.clone()),
                                name: name.clone(),
                                ty,
                            };
                            let tgt = HirAssignTarget::Member {
                                object: object_hir,
                                name,
                            };
                            let assign = HirExpr::Assign {
                                target: Box::new(tgt),
                                value: Box::new(value),
                            };
                            return Ok(HirExpr::Logical {
                                op: lop,
                                lhs: Box::new(current_val),
                                rhs: Box::new(assign),
                            });
                        }
                    }
                    if !matches!(op, AssignOp::Assign) {
                        let bop = compound_to_bin(*op)?;
                        let ty = numeric_ty(self.ann, target.range.start.offset);
                        let object_hir = self.lower_expr(object, scope)?;
                        if *computed {
                            let index = self.lower_expr(property, scope)?;
                            let value = self.lower_expr(value, scope)?;
                            let is_arr = self.ann.get_array_index(target.range.start.offset);
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
                            let tgt = HirAssignTarget::Index {
                                object: object_hir,
                                index,
                                is_array: is_arr,
                            };
                            return Ok(HirExpr::Assign {
                                target: Box::new(tgt),
                                value: Box::new(new_val),
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
                            let value = self.lower_expr(value, scope)?;
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
                            let tgt = HirAssignTarget::Member {
                                object: object_hir,
                                name,
                            };
                            return Ok(HirExpr::Assign {
                                target: Box::new(tgt),
                                value: Box::new(new_val),
                            });
                        }
                    }
                    let object_hir = self.lower_expr(object, scope)?;
                    let tgt = if *computed {
                        let index = self.lower_expr(property, scope)?;
                        let is_array = self.ann.get_array_index(target.range.start.offset);
                        HirAssignTarget::Index {
                            object: object_hir,
                            index,
                            is_array,
                        }
                    } else {
                        let name = match &property.kind {
                            ExprKind::Identifier { name } => name.clone(),
                            _ => {
                                return Err(OptError::Unsupported(
                                    "hir: non-identifier property assign",
                                ))
                            }
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
                    _ => return Err(OptError::Unsupported("hir: non-identifier assign target")),
                };
                let val_expr = self.lower_expr(value, scope)?;
                if matches!(
                    op,
                    AssignOp::AndAssign | AssignOp::OrAssign | AssignOp::NullishAssign
                ) {
                    let lop = match op {
                        AssignOp::AndAssign => HirLogicalOp::And,
                        AssignOp::OrAssign => HirLogicalOp::Or,
                        AssignOp::NullishAssign => HirLogicalOp::Nullish,
                        _ => unreachable!(),
                    };
                    let lhs = HirExpr::Var(binding.clone());
                    let assign = HirExpr::Assign {
                        target: Box::new(HirAssignTarget::Var(binding)),
                        value: Box::new(val_expr),
                    };
                    return Ok(HirExpr::Logical {
                        op: lop,
                        lhs: Box::new(lhs),
                        rhs: Box::new(assign),
                    });
                }
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
            ExprKind::Try { expression } => {
                let inner = self.lower_expr(expression, scope)?;
                Ok(HirExpr::TryOp(Box::new(inner)))
            }
            ExprKind::Is {
                expression,
                type_ann,
            } => {
                let value = Box::new(self.lower_expr(expression, scope)?);
                Ok(HirExpr::TypeTest {
                    value,
                    kind: self.type_test_of(type_ann),
                })
            }
            ExprKind::Pipeline { left, right } => {
                if pipeline_has_placeholder(right) {
                    let range = varn_core::SourceRange::default();
                    let param = varn_core::ast::Param {
                        pattern: varn_core::ast::Pattern::Identifier {
                            name: Rc::from("_"),
                            type_ann: None,
                            range,
                        },
                        type_ann: None,
                        default: None,
                        is_rest: false,
                        is_optional: false,
                        modifiers: varn_core::ast::operators::Modifiers::default(),
                        range,
                    };
                    let body_expr = right.clone();
                    let body_ref = BodyRef::ExprBody(&body_expr);
                    let (func, upvalues) = self.lower_function_like(
                        Rc::from("<pipe>"),
                        &[param],
                        false,
                        false,
                        false,
                        false,
                        body_ref,
                        &[],
                        scope,
                    )?;
                    let callee = HirExpr::Closure {
                        func: Box::new(func),
                        upvalues,
                    };
                    let arg = self.lower_expr(left, scope)?;
                    Ok(HirExpr::Call {
                        callee: Box::new(callee),
                        args: vec![arg],
                        ty: HirType::Dynamic,
                    })
                } else {
                    let callee = Box::new(self.lower_expr(right, scope)?);
                    let arg = self.lower_expr(left, scope)?;
                    Ok(HirExpr::Call {
                        callee,
                        args: vec![arg],
                        ty: HirType::Dynamic,
                    })
                }
            }
            ExprKind::TaggedTemplate { tag, template, .. } => {
                let htag = self.lower_expr(tag, scope)?;
                let htpl = self.lower_expr(template, scope)?;
                Ok(HirExpr::TaggedTemplate {
                    tag: Box::new(htag),
                    template: Box::new(htpl),
                })
            }
            ExprKind::Super => Ok(HirExpr::Super),
            ExprKind::ClassExpr { declaration } => {
                let hir_class = self.lower_class(declaration, scope)?;
                Ok(HirExpr::Class(Box::new(hir_class)))
            }
            _ => Err(OptError::Unsupported("hir: expression kind")),
        }
    }

    fn is_self_call(&self, callee: &Expr, scope: &Scope) -> bool {
        let ExprKind::Identifier { name } = &callee.kind else {
            return false;
        };
        match &self.current_fn {
            Some(cur) if cur == name => {}
            _ => return false,
        }

        scope.resolve_in_current_frame(name).is_none() && !self.ann.is_reassigned_name(name)
    }

    fn lower_call_args(&mut self, args: &[Arg], offset: u32, scope: &mut Scope) -> R<Vec<HirExpr>> {
        if let Some(mapping) = self.ann.get_call_mapping(offset).cloned() {
            let mut out = Vec::with_capacity(mapping.len());
            for opt in &mapping {
                match opt {
                    Some(i) => match &args[*i] {
                        Arg::Positional(e) | Arg::Named { value: e, .. } => {
                            out.push(self.lower_expr(e, scope)?);
                        }
                        Arg::Spread(e) => {
                            let h = self.lower_expr(e, scope)?;
                            out.push(HirExpr::Spread(Box::new(h)));
                        }
                    },
                    None => out.push(HirExpr::Null),
                }
            }
            Ok(out)
        } else {
            let mut out = Vec::with_capacity(args.len());
            for a in args {
                match a {
                    Arg::Positional(e) | Arg::Named { value: e, .. } => {
                        out.push(self.lower_expr(e, scope)?)
                    }
                    Arg::Spread(e) => {
                        let h = self.lower_expr(e, scope)?;
                        out.push(HirExpr::Spread(Box::new(h)));
                    }
                }
            }
            Ok(out)
        }
    }

    pub(super) fn ext_global_call(
        &self,
        mangled: Rc<str>,
        recv: HirExpr,
        args: Vec<HirExpr>,
    ) -> HirExpr {
        HirExpr::ExtensionCall {
            func: mangled,
            recv: Box::new(recv),
            args,
        }
    }

    pub(super) fn resolve(&self, name: &Rc<str>, scope: &mut Scope) -> HirBinding {
        if let Some(b) = scope.resolve(name) {
            b
        } else {
            self.global_binding(name.clone())
        }
    }

    pub(super) fn global_binding(&self, name: Rc<str>) -> HirBinding {
        if self.local_globals.contains(&name) {
            let qualified = format!("{}::{}", self.source_file, name);
            HirBinding::Global(Rc::from(qualified))
        } else {
            HirBinding::Global(name)
        }
    }

    fn type_test_of(&self, type_ann: &varn_core::ast::types::TypeNode) -> HirTypeTest {
        use varn_core::{IntrinsicType, TypeKind, TypeTag};
        match &type_ann.kind {
            TypeKind::Intrinsic(TypeTag::Null) => HirTypeTest::IsNull,
            TypeKind::Intrinsic(TypeTag::Array) | TypeKind::Array(_) => HirTypeTest::IsArray,
            TypeKind::Generic(n, _, _) if n.as_str() == IntrinsicType::Array.as_str() => {
                HirTypeTest::IsArray
            }
            TypeKind::Intrinsic(tt) => {
                HirTypeTest::TypeofEq(Rc::from(IntrinsicType::from(*tt).as_str()))
            }
            TypeKind::Named(name, _) => match IntrinsicType::from_str(name) {
                Some(it) if it.is_scalar_primitive() => HirTypeTest::TypeofEq(Rc::from(it.as_str())),
                _ => {
                    let name_rc = Rc::from(name.as_str());
                    let binding = self.global_binding(name_rc);
                    let final_name = match binding {
                        HirBinding::Global(n) => n,
                        _ => Rc::from(name.as_str()),
                    };
                    HirTypeTest::Instanceof(final_name)
                }
            },
            _ => HirTypeTest::AlwaysFalse,
        }
    }
}

fn pipeline_has_placeholder(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Identifier { name } => &**name == "_",
        ExprKind::Call { callee, args, .. } => {
            pipeline_has_placeholder(callee)
                || args.iter().any(|a| match a {
                    Arg::Positional(e) | Arg::Spread(e) => pipeline_has_placeholder(e),
                    Arg::Named { value, .. } => pipeline_has_placeholder(value),
                })
        }
        ExprKind::Member { object, .. } => pipeline_has_placeholder(object),
        ExprKind::Paren { expression } => pipeline_has_placeholder(expression),
        ExprKind::Binary { left, right, .. } | ExprKind::Logical { left, right, .. } => {
            pipeline_has_placeholder(left) || pipeline_has_placeholder(right)
        }
        ExprKind::Unary { operand, .. } => pipeline_has_placeholder(operand),
        ExprKind::Conditional {
            test,
            consequent,
            alternate,
        } => {
            pipeline_has_placeholder(test)
                || pipeline_has_placeholder(consequent)
                || pipeline_has_placeholder(alternate)
        }
        ExprKind::Array { elements, .. } => elements.iter().any(|el| match el {
            ArrayEl::Expr(e) | ArrayEl::Spread(e) => pipeline_has_placeholder(e),
            ArrayEl::Hole => false,
        }),
        _ => false,
    }
}
