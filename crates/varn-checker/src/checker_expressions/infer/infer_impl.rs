use crate::binder::BindResult;
use crate::checker::Checker;
use crate::types::TypeContext;
use crate::types::{ObjectTypeMember, Type};
use std::rc::Rc;
use varn_core::ast::{Expr, ExprKind};
use varn_core::{Diagnostic, ErrorCode, IntrinsicType, TypeKind, TypeTag};

use super::member_binary::{infer_binary_type, infer_member_type};

impl Checker {
    pub(super) fn infer_type_impl(&mut self, expr: &Expr, bind: &BindResult) -> Type {
        match &expr.kind {
            ExprKind::Identifier { name } => {
                let scope = bind.scopes.get(self.current_scope);
                if let Some(sid) = scope.resolve(name.as_ref(), &bind.scopes) {
                    if let Some(ty) = self.symbol_types.get(&sid) {
                        return ty.clone();
                    }
                    if let Some(ty) = bind.arena.get(sid).ty.clone() {
                        return ty;
                    }
                }
                bind.resolve_symbol(name.as_ref()).unwrap_or(Type::Dynamic)
            }
            ExprKind::This => self
                .current_class
                .as_ref()
                .map(|cn| match IntrinsicType::from_str(cn) {
                    // Extensions on a primitive set `current_class` to the
                    // primitive's name; `this` is then that intrinsic type, not
                    // a named type, so `return this` stays compatible with the
                    // primitive's declared return type (fixes WR3001 on e.g.
                    // `extension X on int { f(): int { return this } }`).
                    Some(it) if it.is_scalar_primitive() => Type::intrinsic(it.0),
                    _ => Type::named(cn.to_string()),
                })
                .unwrap_or(Type::Dynamic),
            ExprKind::New {
                callee, type_args, ..
            } => {
                let callee_ty = self.infer_type(callee, bind);
                match &callee_ty.0 {
                    TypeKind::Named(name, origin) => {
                        if !type_args.is_empty() {
                            let args: Vec<Type> = type_args
                                .iter()
                                .map(|a| self.resolve_type_node_cached(a, bind))
                                .collect();
                            Type::generic_with_origin(
                                name.to_string(),
                                args,
                                origin.as_ref().map(|s| s.to_string()),
                            )
                        } else {
                            Type::named_with_origin(
                                name.to_string(),
                                origin.as_ref().map(|s| s.to_string()),
                            )
                        }
                    }
                    TypeKind::Generic(name, args, origin) => {
                        Type::generic_with_origin(
                            name.to_string(),
                            args.clone(),
                            origin.as_ref().map(|s| s.to_string()),
                        )
                    }
                    _ => {
                        if let ExprKind::Identifier { name } = &callee.kind {
                            if !type_args.is_empty() {
                                let args: Vec<Type> = type_args
                                    .iter()
                                    .map(|a| self.resolve_type_node_cached(a, bind))
                                    .collect();
                                Type::generic(name.to_string(), args)
                            } else {
                                Type::named(name.to_string())
                            }
                        } else {
                            Type::Dynamic
                        }
                    }
                }
            }
            ExprKind::Call { .. } => self.infer_call_type(expr, bind),
            ExprKind::Conditional {
                consequent,
                alternate,
                ..
            } => {
                let t_ty = self.infer_type(consequent, bind);
                let f_ty = self.infer_type(alternate, bind);
                if self.source_file.as_ref() != bind.source_file.as_ref() {
                    t_ty
                } else if t_ty.is_dynamic() {
                    f_ty
                } else if f_ty.is_dynamic() {
                    t_ty
                } else if t_ty == f_ty {
                    t_ty
                } else {
                    Type::union(vec![t_ty, f_ty])
                }
            }
            ExprKind::Member {
                object,
                property,
                computed,
                ..
            } => {
                if !*computed {
                    infer_member_type(self, expr, object, property, bind)
                } else {
                    self.infer_computed_member(object, property, expr, bind)
                }
            }
            ExprKind::Arrow {
                params,
                return_type,
                body,
                ..
            } => self.infer_arrow_type(expr, params, return_type, body, bind),
            ExprKind::Object { properties } => self.infer_object_type(properties, bind, expr),
            ExprKind::As {
                expression,
                type_ann,
                ..
            } => {
                self.check_expr(expression, bind);
                self.resolve_type_node_cached(type_ann, bind)
            }
            ExprKind::Satisfies {
                expression,
                type_ann,
                ..
            } => {
                let ty = self.infer_type(expression, bind);
                let target = self.resolve_type_node_cached(type_ann, bind);
                if !self.types_compatible_cached(&target, &ty, Some(bind)) {
                    self.emit(
                        Diagnostic::error(
                            ErrorCode::InvalidSatisfies,
                            format!("type '{ty}' does not satisfy '{target}'"),
                        )
                        .with_range(expression.range),
                    );
                }
                ty
            }
            ExprKind::Await { argument } => {
                let inner = self.infer_type(argument, bind);
                match &inner.0 {
                    TypeKind::Generic(name, args, _)
                        if (name.as_ref() == IntrinsicType::Task.as_str()
                            || name.as_ref() == IntrinsicType::TaskHandle.as_str())
                            && args.len() == 1 =>
                    {
                        args[0].clone()
                    }
                    _ => inner,
                }
            }
            ExprKind::NonNull { expression } => {
                let ty = self.infer_type(expression, bind);
                if let TypeKind::Union(members) = &ty.0 {
                    let filtered: Vec<Type> = members
                        .iter()
                        .filter(|m| {
                            !matches!(
                                m.0,
                                TypeKind::Intrinsic(TypeTag::Null)
                                    | TypeKind::Intrinsic(TypeTag::Void)
                            )
                        })
                        .cloned()
                        .collect();
                    if filtered.len() == 1 {
                        return filtered[0].clone();
                    }
                    return Type::union(filtered);
                }
                ty
            }
            ExprKind::Logical { op, left, right } => {
                let l_ty = self.infer_type(left, bind);
                let r_ty = self.infer_type(right, bind);
                match op {
                    varn_core::ast::LogicalOp::And => {
                        if l_ty == r_ty {
                            l_ty
                        } else {
                            Type::union(vec![l_ty, r_ty])
                        }
                    }
                    varn_core::ast::LogicalOp::Nullish => {
                        let l_non_null = l_ty.non_nullified();
                        if l_non_null == r_ty {
                            r_ty
                        } else {
                            Type::union(vec![l_non_null, r_ty])
                        }
                    }
                    varn_core::ast::LogicalOp::Or => {
                        if l_ty == r_ty {
                            l_ty
                        } else {
                            Type::union(vec![l_ty, r_ty])
                        }
                    }
                }
            }
            ExprKind::Binary { op, left, right } => infer_binary_type(self, op, left, right, bind),
            ExprKind::Unary { op, operand, .. } => match op {
                varn_core::ast::operators::UnaryOp::Not => Type::Bool,
                varn_core::ast::operators::UnaryOp::Minus
                | varn_core::ast::operators::UnaryOp::Plus => self.infer_type(operand, bind),
                varn_core::ast::operators::UnaryOp::Typeof => Type::Str,
                varn_core::ast::operators::UnaryOp::BitNot => {
                    let inner = self.infer_type(operand, bind);
                    match &inner.0 {
                        TypeKind::Intrinsic(TypeTag::Int) => Type::intrinsic(TypeTag::Int),
                        _ => Type::Dynamic,
                    }
                }
            },
            ExprKind::Update { operand, .. } => self.infer_type(operand, bind),
            ExprKind::Assign { value, .. } => self.infer_type(value, bind),
            ExprKind::Array { elements } => {
                let mut elem_tys = Vec::new();
                for el in elements {
                    match el {
                        varn_core::ast::ArrayEl::Expr(e) => {
                            let ty = self.infer_type(e, bind);
                            if !ty.is_dynamic() {
                                elem_tys.push(ty);
                            }
                        }
                        varn_core::ast::ArrayEl::Spread(e) => {
                            let ty = self.infer_type(e, bind);
                            if let TypeKind::Array(inner) = &ty.0 {
                                elem_tys.push((**inner).clone());
                            }
                        }
                        _ => {}
                    }
                }
                if elem_tys.is_empty() {
                    if let Some(expected) = &self.expected_type {
                        if let TypeKind::Array(expected_inner) = &expected.non_nullified().0 {
                            Type::array((**expected_inner).clone())
                        } else {
                            Type::array(Type::Dynamic)
                        }
                    } else {
                        Type::array(Type::Dynamic)
                    }
                } else {
                    let first = elem_tys[0].clone();
                    if elem_tys.iter().all(|t| t == &first) {
                        Type::array(widen_literal(first))
                    } else {
                        Type::array(widen_literal(Type::union(elem_tys)))
                    }
                }
            }
            ExprKind::Template { .. } => Type::Str,
            ExprKind::Paren { expression } => self.infer_type(expression, bind),
            ExprKind::IntLiteral { .. } => Type::Int,
            ExprKind::FloatLiteral { .. } => Type::Float,
            ExprKind::DecimalLiteral { .. } => Type::Decimal,
            ExprKind::BigIntLiteral { .. } => Type::BigInt,
            ExprKind::StrLiteral { value, .. } => Type::literal_str(value.to_string()),
            ExprKind::CharLiteral { .. } => Type::Char,
            ExprKind::BoolLiteral { .. } => Type::Bool,
            ExprKind::NullLiteral => Type::Null,
            ExprKind::Range { .. } => Type::intrinsic(varn_core::TypeTag::Range),
            ExprKind::Match { cases, .. } => {
                let mut tys = Vec::new();
                for case in cases {
                    match &case.body {
                        varn_core::ast::MatchBody::Expr(e) => {
                            let ty = self.infer_type(e, bind);
                            tys.push(ty);
                        }
                        varn_core::ast::MatchBody::Block(_) => {
                            tys.push(Type::Void);
                        }
                    }
                }
                if tys.is_empty() {
                    Type::Dynamic
                } else {
                    let first = tys[0].clone();
                    if tys.iter().all(|t| t == &first) {
                        first
                    } else {
                        Type::union(tys)
                    }
                }
            }
            ExprKind::Pipeline { left, right } => {
                let lhs_ty = self.infer_type(left, bind);
                let saved_pipeline = self.in_pipeline_rhs;
                let saved_pipe_ty = self.pipeline_value_type.replace(lhs_ty);
                self.in_pipeline_rhs = true;
                let res = self.infer_type(right, bind);
                self.in_pipeline_rhs = saved_pipeline;
                self.pipeline_value_type = saved_pipe_ty;
                match &res.0 {
                    TypeKind::Fn(ft) => *ft.return_type.clone(),
                    _ => res,
                }
            }
            _ => Type::Dynamic,
        }
    }

    pub(crate) fn infer_computed_member(
        &mut self,
        object: &Expr,
        property: &Expr,
        _expr: &Expr,
        bind: &BindResult,
    ) -> Type {
        let obj_ty = self.infer_type(object, bind);
        let prop_ty = self.infer_type(property, bind);
        match &obj_ty.0 {
            TypeKind::Array(inner) if matches!(prop_ty.0, TypeKind::Intrinsic(TypeTag::Int)) => {
                (**inner).clone()
            }
            TypeKind::Intrinsic(TypeTag::Str) if matches!(prop_ty.0, TypeKind::Intrinsic(TypeTag::Int)) => {
                Type::Str
            }
            TypeKind::Named(name, _)
                if name.as_ref() == "str" && matches!(prop_ty.0, TypeKind::Intrinsic(TypeTag::Int)) =>
            {
                Type::Str
            }
            _ => Type::Dynamic,
        }
    }

    pub(crate) fn infer_object_type(
        &mut self,
        properties: &[varn_core::ast::ObjectProp],
        bind: &BindResult,
        _expr: &Expr,
    ) -> Type {
        let mut members = Vec::new();
        for prop in properties {
            match prop {
                varn_core::ast::ObjectProp::Property { key, value, .. } => {
                    let name = match key {
                        varn_core::ast::expr::PropKey::Identifier(n) => n.clone(),
                        varn_core::ast::expr::PropKey::Str(s) => s.clone(),
                        _ => continue,
                    };
                    let ty = self.infer_type(value, bind);
                    members.push(ObjectTypeMember::Property {
                        name: Rc::from(name.as_str()),
                        ty,
                        optional: false,
                        readonly: false,
                    });
                }
                varn_core::ast::ObjectProp::Spread { argument, .. } => {
                    let spread_ty = self.infer_type(argument, bind);
                    if let varn_core::TypeKind::Object(spread_members) = &spread_ty.0 {
                        for m in spread_members {
                            members.push(m.clone());
                        }
                    }
                }
                _ => {}
            }
        }
        Type::object(members)
    }
}

fn widen_literal(ty: Type) -> Type {
    crate::binder::widen_literal(ty)
}
