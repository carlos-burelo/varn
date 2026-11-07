use crate::checker::recorder::Recorder;
use crate::checker::Checker;
use varn_core::ast::{ExprId, ExprKind};
use varn_core::TypeKind;
use varn_sem::bind::BindResult;
use varn_sem::types::{Type, TypeContext};

use super::member_binary::{infer_binary_type, infer_member_type};

impl<'r> Checker<'r> {
    pub(super) fn infer_type_impl(
        &mut self,
        rec: &mut Recorder,
        expr: ExprId,
        bind: &BindResult,
    ) -> Type {
        let arena = self.ast_arena;
        match &arena.expr(expr).kind {
            ExprKind::Identifier { name } => {
                let name_str = bind.interner.resolve(*name);

                if name_str == varn_core::TokenKind::Placeholder.as_str() && self.in_pipeline_rhs {
                    return self.pipeline_value_type.unwrap_or(Type::Error);
                }
                let scope = bind.scopes.get(self.current_scope);
                if let Some(sid) = scope.resolve(*name, &bind.scopes) {
                    if let Some(ty) = rec.symbol_types.get(&sid) {
                        return *ty;
                    }
                    if let Some(ty) = bind.arena.get(sid).ty {
                        if bind.arena.get(sid).kind == varn_sem::symbol::SymbolKind::Class {
                            return Type::builtin(
                                varn_core::BuiltinType::Class,
                                vec![ty],
                                &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                            );
                        }
                        return ty;
                    }
                }
                varn_sem::bind::BindView::new(bind, self.resolver)
                    .resolve_symbol(name_str)
                    .unwrap_or(Type::Error)
            }
            ExprKind::ImportMeta(varn_core::ast::MetaProperty::Main) => Type::Bool,
            ExprKind::This => self
                .extension_self
                .or_else(|| {
                    self.current_class.clone().map(|cn| {
                        Type::named_with_origin(
                            cn.to_string(),
                            Some(bind.source_file.clone()),
                            &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                        )
                    })
                })
                .unwrap_or(Type::Error),

            ExprKind::Super => self
                .current_class
                .as_ref()
                .and_then(|cn| bind.class_parents.get(cn))
                .cloned()
                .map(|parent| self.parent_type(&parent))
                .unwrap_or(Type::Error),
            ExprKind::New {
                callee,
                type_args,
                args,
                ..
            } => self.infer_new_type(rec, expr, *callee, type_args, args, bind),
            ExprKind::Call { .. } => self.infer_call_type(rec, expr, bind),
            ExprKind::TaggedTemplate { tag, .. } => {
                let tag_ty = self.infer_type(rec, *tag, bind);
                let tag_ty =
                    tag_ty.non_nullified(&mut *std::sync::Arc::make_mut(&mut self.ty_table));
                if let TypeKind::Fn(fid) = self.ty_table.get(tag_ty.0) {
                    let ret = self.ty_table.get_function(fid).return_type;
                    Type::resolved(ret)
                } else {
                    Type::Dynamic
                }
            }
            ExprKind::With { object, .. } => self.infer_type(rec, *object, bind),
            ExprKind::Conditional {
                test,
                consequent,
                alternate,
            } => {
                let (test, consequent, alternate) = (*test, *consequent, *alternate);
                let t_ty = self.in_branch(rec, test, bind, true, |c, rec| {
                    c.infer_type(rec, consequent, bind)
                });
                let f_ty = self.in_branch(rec, test, bind, false, |c, rec| {
                    c.infer_type(rec, alternate, bind)
                });
                let is_empty_object = |e: ExprId| matches!(&arena.expr(e).kind, ExprKind::Object { properties } if properties.is_empty());
                if self.source_file.as_ref() != bind.source_file.as_ref() {
                    t_ty
                } else if is_empty_object(consequent) && !is_empty_object(alternate) {
                    f_ty
                } else if is_empty_object(alternate) && !is_empty_object(consequent) {
                    t_ty
                } else if t_ty.is_unresolved() {
                    f_ty
                } else if f_ty.is_unresolved() || t_ty == f_ty {
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
                    infer_member_type(self, rec, expr, object, property, bind)
                } else {
                    self.infer_computed_member(rec, object, property, expr, bind)
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
                self.infer_arrow_type(rec, expr, &params, &return_type, body, is_async, bind)
            }
            ExprKind::Function {
                params,
                return_type,
                is_async,
                is_generator,
                body,
                ..
            } => self.infer_function_expr_type(
                rec,
                params,
                return_type,
                *body,
                *is_async,
                *is_generator,
                bind,
            ),
            ExprKind::Object { properties } => {
                let properties = properties.clone();
                self.infer_object_type(rec, &properties, bind, expr)
            }
            ExprKind::Tuple { elements } => self.infer_tuple(rec, elements, bind),
            ExprKind::As {
                expression,
                type_ann,
                ..
            } => {
                let (expression, type_ann) = (*expression, type_ann.clone());
                self.check_expr(rec, expression, bind);
                self.resolve_type_node_cached(&type_ann, bind)
            }
            ExprKind::Satisfies { expression, .. } => {
                let expression = *expression;
                self.infer_type(rec, expression, bind)
            }
            ExprKind::MetaAccess { target, property } => {
                self.infer_meta_access(rec, *target, *property, bind)
            }
            ExprKind::Await { argument } => {
                let inner = self.infer_type(rec, *argument, bind);
                varn_sem::types::awaited(&inner, &self.ty_table)
            }
            ExprKind::NonNull { expression } => self.infer_non_null(rec, *expression, bind),
            ExprKind::Try { expression } => self.infer_try(rec, *expression, bind),
            ExprKind::Logical { op, left, right } => {
                self.infer_logical(rec, *op, *left, *right, bind)
            }
            ExprKind::Binary { op, left, right } => {
                let (op, left, right) = (*op, *left, *right);
                let capability = varn_core::capability::binary_operator_method(op).and_then(|m| {
                    let l = self.infer_type(rec, left, bind);
                    self.resolve_operator(&l, m, bind)
                });
                match capability {
                    Some(resolved) => resolved.result,
                    None => infer_binary_type(self, rec, op, left, right, bind),
                }
            }
            ExprKind::Unary { op, operand, .. } => {
                let (op, operand) = (*op, *operand);
                match op {
                    varn_core::ast::operators::UnaryOp::Not => Type::Bool,
                    varn_core::ast::operators::UnaryOp::Minus => {
                        let inner = self.infer_type(rec, operand, bind);
                        varn_core::capability::unary_operator_method(op)
                            .and_then(|m| self.resolve_operator(&inner, m, bind))
                            .map_or(inner, |resolved| resolved.result)
                    }
                    varn_core::ast::operators::UnaryOp::Plus => self.infer_type(rec, operand, bind),
                    varn_core::ast::operators::UnaryOp::Typeof => Type::Str,
                    varn_core::ast::operators::UnaryOp::BitNot => {
                        let inner = self.infer_type(rec, operand, bind);
                        if inner.is_int() {
                            Type::primitive(
                                varn_core::LangPrimitive::Int,
                                &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                            )
                        } else {
                            Type::Error
                        }
                    }
                }
            }
            ExprKind::Update { operand, .. } => self.infer_type(rec, *operand, bind),
            ExprKind::Assign { value, .. } => self.infer_type(rec, *value, bind),
            ExprKind::Array { elements } => self.infer_array(rec, expr, elements, bind),
            ExprKind::Template { .. } => Type::Str,
            ExprKind::Paren { expression } => self.infer_type(rec, *expression, bind),
            ExprKind::IntLiteral { .. } => Type::Int,
            ExprKind::FloatLiteral { .. } => Type::Float,
            ExprKind::DecimalLiteral { .. } => Type::Decimal,
            ExprKind::BigIntLiteral { .. } => Type::BigInt,
            ExprKind::StrLiteral { .. } => Type::Str,
            ExprKind::CharLiteral { .. } => Type::Char,
            ExprKind::BoolLiteral { .. } => Type::Bool,
            ExprKind::NullLiteral => Type::Null,
            ExprKind::Range { start, .. } => {
                let bound = self.infer_type(rec, *start, bind);
                Type::range_over(&bound, &mut *std::sync::Arc::make_mut(&mut self.ty_table))
            }
            ExprKind::Match { subject, cases } => {
                self.infer_match(rec, *subject, cases, arena, bind)
            }
            ExprKind::Pipeline { left, right } => {
                let (left, right) = (*left, *right);
                let lhs_ty = self.infer_type(rec, left, bind);
                let saved_pipeline = self.in_pipeline_rhs;
                let saved_pipe_ty = self.pipeline_value_type.replace(lhs_ty);
                self.in_pipeline_rhs = true;
                let res = self.infer_type(rec, right, bind);
                self.in_pipeline_rhs = saved_pipeline;
                self.pipeline_value_type = saved_pipe_ty;
                match self.ty_table.get(res.0) {
                    TypeKind::Fn(fid) => {
                        Type::resolved(self.ty_table.get_function(fid).return_type)
                    }
                    TypeKind::Primitive(_)
                    | TypeKind::Literal(_)
                    | TypeKind::This
                    | TypeKind::Array(_)
                    | TypeKind::Union(_)
                    | TypeKind::Intersection(_)
                    | TypeKind::Tuple(_)
                    | TypeKind::Named(..)
                    | TypeKind::Generic(..)
                    | TypeKind::TemplateLiteral(_)
                    | TypeKind::Record(_)
                    | TypeKind::Interface(_)
                    | TypeKind::Typeof(_)
                    | TypeKind::KeyOf(_)
                    | TypeKind::IndexedAccess { .. }
                    | TypeKind::Mapped { .. }
                    | TypeKind::Conditional { .. }
                    | TypeKind::Infer(_)
                    | TypeKind::EnumVariant { .. }
                    | TypeKind::TypePredicate { .. } => res,
                }
            }

            ExprKind::Missing => Type::Error,
            ExprKind::Is { .. } => Type::Bool,
            ExprKind::Sequence { expressions } => match expressions.last() {
                Some(last) => self.infer_type(rec, *last, bind),
                None => Type::Dynamic,
            },
            ExprKind::Yield { .. } => Type::Void,
            ExprKind::Spawn { .. } | ExprKind::Spread { .. } | ExprKind::ClassExpr { .. } => {
                Type::Dynamic
            }
        }
    }
}
