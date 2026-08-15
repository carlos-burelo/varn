use rust_decimal::Decimal;
use std::rc::Rc;

use std::str::FromStr;

use varn_core::ast::expr::{ArrayEl, TemplatePart};
use varn_core::ast::operators::UnaryOp;
use varn_core::ast::{Arg, Expr, ExprKind};

use super::*;

mod assign;
mod calls;
mod collections;
mod functions;
mod match_expr;

impl<'a> Lowerer<'a> {
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
            ExprKind::Call { .. }
            | ExprKind::Member { .. }
            | ExprKind::Pipeline { .. }
            | ExprKind::TaggedTemplate { .. } => self.lower_call_expr(expr, scope),

            ExprKind::Assign { .. } | ExprKind::Update { .. } => {
                self.lower_assign_expr(expr, scope)
            }

            ExprKind::Array { .. }
            | ExprKind::Tuple { .. }
            | ExprKind::Record { .. }
            | ExprKind::Object { .. } => self.lower_collection_expr(expr, scope),

            ExprKind::Function { .. } | ExprKind::Arrow { .. } | ExprKind::ClassExpr { .. } => {
                self.lower_function_expr(expr, scope)
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
            ExprKind::Super => Ok(HirExpr::Super),
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
                Some(it) if it.is_scalar_primitive() => {
                    HirTypeTest::TypeofEq(Rc::from(it.as_str()))
                }
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
