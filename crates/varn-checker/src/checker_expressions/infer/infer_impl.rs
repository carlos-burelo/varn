use crate::binder::BindResult;
use crate::checker::Checker;
use crate::types::{Type, TypeContext};
use varn_core::ast::{ExprId, ExprKind};
use varn_core::{Diagnostic, ErrorCode, TypeKind};

use super::member_binary::{infer_binary_type, infer_member_type};

impl<'r> Checker<'r> {
    pub(super) fn infer_type_impl(&mut self, expr: ExprId, bind: &BindResult) -> Type {
        let arena = self.ast_arena;
        match &arena.expr(expr).kind {
            ExprKind::Identifier { name } => {
                let name_str = bind.interner.resolve(*name);

                if name_str == "_" && self.in_pipeline_rhs {
                    return self.pipeline_value_type.unwrap_or(Type::Dynamic);
                }
                let scope = bind.scopes.get(self.current_scope);
                if let Some(sid) = scope.resolve(*name, &bind.scopes) {
                    if let Some(ty) = self.symbol_types.get(&sid) {
                        return *ty;
                    }
                    if let Some(ty) = bind.arena.get(sid).ty {
                        return ty;
                    }
                }
                crate::binder::BindView::new(bind, self.resolver)
                    .resolve_symbol(name_str)
                    .unwrap_or(Type::Dynamic)
            }
            ExprKind::This => self
                .current_class
                .as_ref()
                .map(|cn| match varn_core::LangPrimitive::from_str(cn) {
                    Some(p) if p != varn_core::LangPrimitive::Dynamic => {
                        Type::primitive(p, &mut *std::sync::Arc::make_mut(&mut self.ty_table))
                    }
                    _ => Type::named(
                        cn.to_string(),
                        &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                    ),
                })
                .unwrap_or(Type::Dynamic),

            ExprKind::Super => self
                .current_class
                .as_ref()
                .and_then(|cn| bind.class_parents.get(cn))
                .cloned()
                .map(|parent| self.parent_type(&parent))
                .unwrap_or(Type::Dynamic),
            ExprKind::New {
                callee, type_args, ..
            } => self.infer_new_type(*callee, type_args, bind),
            ExprKind::Call { .. } => self.infer_call_type(expr, bind),
            ExprKind::TaggedTemplate { tag, .. } => {
                let tag_ty = self.infer_type(*tag, bind);
                let tag_ty =
                    tag_ty.non_nullified(&mut *std::sync::Arc::make_mut(&mut self.ty_table));
                if let TypeKind::Fn(fid) = self.ty_table.get(tag_ty.0) {
                    let ret = self.ty_table.get_function(fid).return_type;
                    Type::resolved(ret)
                } else {
                    Type::Dynamic
                }
            }
            ExprKind::With { object, .. } => self.infer_type(*object, bind),
            ExprKind::Conditional {
                consequent,
                alternate,
                ..
            } => {
                let (consequent, alternate) = (*consequent, *alternate);
                let t_ty = self.infer_type(consequent, bind);
                let f_ty = self.infer_type(alternate, bind);
                if self.source_file.as_ref() != bind.source_file.as_ref() {
                    t_ty
                } else if t_ty.is_dynamic() {
                    f_ty
                } else if f_ty.is_dynamic() || t_ty == f_ty {
                    t_ty
                } else {
                    Type::union(
                        vec![t_ty, f_ty],
                        &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                    )
                }
            }
            ExprKind::Member {
                object,
                property,
                computed,
                ..
            } => {
                let (object, property, computed) = (*object, *property, *computed);
                if !computed {
                    infer_member_type(self, expr, object, property, bind)
                } else {
                    self.infer_computed_member(object, property, expr, bind)
                }
            }
            ExprKind::Arrow {
                params,
                return_type,
                body,
                is_async,
            } => {
                let (params, return_type, body, is_async) = (
                    params.clone(),
                    return_type.clone(),
                    (**body).clone(),
                    *is_async,
                );
                self.infer_arrow_type(expr, &params, &return_type, body, is_async, bind)
            }
            ExprKind::Function {
                params,
                return_type,
                is_async,
                is_generator,
                ..
            } => self.infer_function_expr_type(params, return_type, *is_async, *is_generator, bind),
            ExprKind::Object { properties } => {
                let properties = properties.clone();
                self.infer_object_type(&properties, bind, expr)
            }
            ExprKind::Tuple { elements } => self.infer_tuple(elements, bind),
            ExprKind::Record { properties } => self.infer_record(properties, bind),
            ExprKind::As {
                expression,
                type_ann,
                ..
            } => {
                let (expression, type_ann) = (*expression, type_ann.clone());
                self.check_expr(expression, bind);
                self.resolve_type_node_cached(&type_ann, bind)
            }
            ExprKind::Satisfies {
                expression,
                type_ann,
                ..
            } => {
                let (expression, type_ann) = (*expression, type_ann.clone());
                let ty = self.infer_type(expression, bind);
                let target = self.resolve_type_node_cached(&type_ann, bind);
                if !self.types_compatible_cached(&target, &ty, Some(bind)) {
                    let range = arena.expr(expression).range;
                    self.emit(
                        Diagnostic::error(
                            ErrorCode::InvalidSatisfies,
                            format!(
                                "type '{}' does not satisfy '{}'",
                                ty.display(&self.ty_table, &bind.interner),
                                target.display(&self.ty_table, &bind.interner)
                            ),
                        )
                        .with_range(range),
                    );
                }
                ty
            }
            ExprKind::MetaAccess { target, property } => {
                self.infer_meta_access(*target, *property, bind)
            }
            ExprKind::Await { argument } => {
                let inner = self.infer_type(*argument, bind);
                crate::types::awaited(&inner, &self.ty_table, &bind.interner)
            }
            ExprKind::NonNull { expression } => self.infer_non_null(*expression, bind),
            ExprKind::Try { expression } => self.infer_try(*expression, bind),
            ExprKind::Logical { op, left, right } => self.infer_logical(*op, *left, *right, bind),
            ExprKind::Binary { op, left, right } => {
                let (op, left, right) = (*op, *left, *right);
                let capability = varn_core::capability::binary_operator_method(op).and_then(|m| {
                    let l = self.infer_type(left, bind);
                    self.resolve_operator(&l, m, bind)
                });
                match capability {
                    Some(resolved) => resolved.result,
                    None => infer_binary_type(self, op, left, right, bind),
                }
            }
            ExprKind::Unary { op, operand, .. } => {
                let (op, operand) = (*op, *operand);
                match op {
                    varn_core::ast::operators::UnaryOp::Not => Type::Bool,
                    varn_core::ast::operators::UnaryOp::Minus => {
                        let inner = self.infer_type(operand, bind);
                        varn_core::capability::unary_operator_method(op)
                            .and_then(|m| self.resolve_operator(&inner, m, bind))
                            .map_or(inner, |resolved| resolved.result)
                    }
                    varn_core::ast::operators::UnaryOp::Plus => self.infer_type(operand, bind),
                    varn_core::ast::operators::UnaryOp::Typeof => Type::Str,
                    varn_core::ast::operators::UnaryOp::BitNot => {
                        let inner = self.infer_type(operand, bind);
                        if inner.is_int() {
                            Type::primitive(
                                varn_core::LangPrimitive::Int,
                                &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                            )
                        } else {
                            Type::Dynamic
                        }
                    }
                }
            }
            ExprKind::Update { operand, .. } => self.infer_type(*operand, bind),
            ExprKind::Assign { value, .. } => self.infer_type(*value, bind),
            ExprKind::Array { elements } => self.infer_array(elements, bind),
            ExprKind::Template { .. } => Type::Str,
            ExprKind::Paren { expression } => self.infer_type(*expression, bind),
            ExprKind::IntLiteral { .. } => Type::Int,
            ExprKind::FloatLiteral { .. } => Type::Float,
            ExprKind::DecimalLiteral { .. } => Type::Decimal,
            ExprKind::BigIntLiteral { .. } => Type::BigInt,
            ExprKind::StrLiteral { .. } => Type::Str,
            ExprKind::CharLiteral { .. } => Type::Char,
            ExprKind::BoolLiteral { .. } => Type::Bool,
            ExprKind::NullLiteral => Type::Null,
            ExprKind::Range { start, .. } => {
                let bound = self.infer_type(*start, bind);
                Type::range_over(&bound, &mut *std::sync::Arc::make_mut(&mut self.ty_table))
            }
            ExprKind::Match { subject, cases } => self.infer_match(*subject, cases, arena, bind),
            ExprKind::Pipeline { left, right } => {
                let (left, right) = (*left, *right);
                let lhs_ty = self.infer_type(left, bind);
                let saved_pipeline = self.in_pipeline_rhs;
                let saved_pipe_ty = self.pipeline_value_type.replace(lhs_ty);
                self.in_pipeline_rhs = true;
                let res = self.infer_type(right, bind);
                self.in_pipeline_rhs = saved_pipeline;
                self.pipeline_value_type = saved_pipe_ty;
                match self.ty_table.get(res.0) {
                    TypeKind::Fn(fid) => {
                        Type::resolved(self.ty_table.get_function(fid).return_type)
                    }
                    _ => res,
                }
            }

            ExprKind::Missing => Type::Dynamic,
            _ => Type::Dynamic,
        }
    }
}
