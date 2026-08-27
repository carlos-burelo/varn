use crate::binder::BindResult;
use crate::checker::Checker;
use crate::types::TypeContext;
use crate::types::{ObjectTypeMember, Type};
use std::rc::Rc;
use varn_core::ast::{Expr, ExprKind};
use varn_core::{Diagnostic, ErrorCode, IntrinsicType, TypeKind, TypeTag};

use super::member_binary::{infer_binary_type, infer_member_type};

impl<'r> Checker<'r> {
    pub(super) fn infer_type_impl(&mut self, expr: &Expr, bind: &BindResult) -> Type {
        match &expr.kind {
            ExprKind::Identifier { name } => {
                // Pipeline placeholder `_` stands for the piped value, so it
                // carries that value's type (`x |> f(_, y)` ⇒ `_` has `x`'s type).
                if name.as_ref() == "_" && self.in_pipeline_rhs {
                    return self.pipeline_value_type.clone().unwrap_or(Type::Dynamic);
                }
                let scope = bind.scopes.get(self.current_scope);
                if let Some(sid) = scope.resolve(name.as_ref(), &bind.scopes) {
                    if let Some(ty) = self.symbol_types.get(&sid) {
                        return ty.clone();
                    }
                    if let Some(ty) = bind.arena.get(sid).ty.clone() {
                        return ty;
                    }
                }
                crate::binder::BindView::new(bind, self.resolver)
                    .resolve_symbol(name.as_ref())
                    .unwrap_or(Type::Dynamic)
            }
            ExprKind::This => self
                .current_class
                .as_ref()
                .map(|cn| match IntrinsicType::from_str(cn) {
                    Some(it) if it.is_scalar_primitive() => Type::intrinsic(it.0),
                    _ => Type::named(cn.to_string()),
                })
                .unwrap_or(Type::Dynamic),

            ExprKind::Super => self
                .current_class
                .as_ref()
                .and_then(|cn| bind.class_parents.get(cn))
                .map(|parent| Type::named(parent.clone()))
                .unwrap_or(Type::Dynamic),
            ExprKind::New {
                callee, type_args, ..
            } => {
                let callee_ty = self.infer_type(callee, bind);
                if callee_ty.is_dynamic() {
                    return Type::Dynamic;
                }
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
                    TypeKind::Generic(name, args, origin) => Type::generic_with_origin(
                        name.to_string(),
                        args.clone(),
                        origin.as_ref().map(|s| s.to_string()),
                    ),
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
            ExprKind::TaggedTemplate { tag, .. } => {
                let tag_ty = self.infer_type(tag, bind).non_nullified();
                if let TypeKind::Fn(ft) = &tag_ty.0 {
                    ft.return_type.as_ref().clone()
                } else {
                    Type::Dynamic
                }
            }
            ExprKind::With { object, .. } => self.infer_type(object, bind),
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
                is_async,
            } => self.infer_arrow_type(expr, params, return_type, body, *is_async, bind),
            ExprKind::Function {
                params,
                return_type,
                is_async,
                is_generator,
                ..
            } => self.infer_function_expr_type(params, return_type, *is_async, *is_generator, bind),
            ExprKind::Object { properties } => self.infer_object_type(properties, bind, expr),
            ExprKind::Tuple { elements } => {
                let elem_tys: Vec<Type> =
                    elements.iter().map(|e| self.infer_type(e, bind)).collect();
                Type(TypeKind::Tuple(elem_tys), false)
            }
            ExprKind::Record { properties } => {
                let mut members = Vec::new();
                for prop in properties {
                    if let varn_core::ast::ObjectProp::Property { key, value, .. } = prop {
                        let ty = self.infer_type(value, bind);
                        let name: std::rc::Rc<str> = match key {
                            varn_core::ast::PropKey::Identifier(s)
                            | varn_core::ast::PropKey::Str(s) => std::rc::Rc::from(s.as_str()),
                            varn_core::ast::PropKey::Int(n) => {
                                std::rc::Rc::from(n.to_string().as_str())
                            }
                            varn_core::ast::PropKey::Computed(_) => std::rc::Rc::from("<computed>"),
                        };
                        members.push(crate::types::ObjectTypeMember::Property {
                            name,
                            ty,
                            optional: false,
                            readonly: true,
                        });
                    }
                }
                Type(TypeKind::Object(members), false)
            }
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
            ExprKind::MetaAccess { target, property } => {
                let _target_ty = self.infer_type(target, bind);
                match varn_core::MemberKey::from_str(property.as_ref()) {
                    Some(varn_core::MemberKey::Name) | Some(varn_core::MemberKey::Type) => {
                        Type::Str
                    }
                    Some(varn_core::MemberKey::Class) => Type::Dynamic,
                    Some(varn_core::MemberKey::Fields) | Some(varn_core::MemberKey::Methods) => {
                        Type::array(Type::Str)
                    }
                    Some(varn_core::MemberKey::Keys) => Type::fn_(crate::types::FunctionType {
                        params: vec![],
                        return_type: Box::new(Type::array(Type::Str)),
                        is_arrow: true,
                        type_params: vec![],
                    }),
                    Some(varn_core::MemberKey::Values) => Type::fn_(crate::types::FunctionType {
                        params: vec![],
                        return_type: Box::new(Type::array(Type::Dynamic)),
                        is_arrow: true,
                        type_params: vec![],
                    }),
                    Some(varn_core::MemberKey::Entries) => {
                        Type::fn_(crate::types::FunctionType {
                            params: vec![],
                            return_type: Box::new(Type::array(Type(
                                TypeKind::Tuple(vec![Type::Str, Type::Dynamic]),
                                false,
                            ))),
                            is_arrow: true,
                            type_params: vec![],
                        })
                    }
                    Some(varn_core::MemberKey::HasOwn) => Type::fn_(crate::types::FunctionType {
                        params: vec![crate::types::FunctionParam {
                            name: Some(std::rc::Rc::from("key")),
                            ty: Type::Str,
                            optional: false,
                            is_rest: false,
                        }],
                        return_type: Box::new(Type::Bool),
                        is_arrow: true,
                        type_params: vec![],
                    }),
                    _ => Type::Dynamic,
                }
            }
            ExprKind::Await { argument } => {
                let inner = self.infer_type(argument, bind);
                crate::types::awaited(&inner)
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
            ExprKind::StrLiteral { .. } => Type::Str,
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
            // A hole the parser left where the source had no expression. The
            // syntax error is already reported; typing it as `Dynamic` lets the
            // enclosing declaration still bind and still answer editor queries.
            ExprKind::Missing => Type::Dynamic,
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
        if matches!(property.kind, varn_core::ast::ExprKind::Range { .. }) {
            return obj_ty;
        }
        let prop_ty = self.infer_type(property, bind);
        match &obj_ty.0 {
            TypeKind::Array(inner) if matches!(prop_ty.0, TypeKind::Intrinsic(TypeTag::Int)) => {
                (**inner).clone()
            }
            TypeKind::Intrinsic(TypeTag::Str)
                if matches!(prop_ty.0, TypeKind::Intrinsic(TypeTag::Int)) =>
            {
                Type::Str
            }
            TypeKind::Named(name, _)
                if name.as_ref() == IntrinsicType::Str.as_str()
                    && matches!(prop_ty.0, TypeKind::Intrinsic(TypeTag::Int)) =>
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
                    let Some(name) = prop_key_name(key) else {
                        continue;
                    };
                    let ty = self.infer_type(value, bind);
                    members.push(ObjectTypeMember::Property {
                        name,
                        ty,
                        optional: false,
                        readonly: false,
                    });
                }
                // A method written in method syntax is a member like any other.
                // Leaving it out made `{ m() {} }` type as `#{ }`, so calling
                // `obj.m()` failed with "property 'm' does not exist".
                varn_core::ast::ObjectProp::Method {
                    key,
                    params,
                    return_type,
                    is_async,
                    ..
                } => {
                    let Some(name) = prop_key_name(key) else {
                        continue;
                    };
                    let ret = return_type
                        .as_ref()
                        .map(|rt| self.resolve_type_node_cached(rt, bind))
                        .unwrap_or(Type::Dynamic);
                    members.push(ObjectTypeMember::Method {
                        name,
                        params: self.signature_params(params, bind),
                        return_type: Box::new(crate::types::async_fn_return(ret, *is_async)),
                        optional: false,
                        is_arrow: false,
                    });
                }
                // Accessors in an object literal contribute no member: the
                // compiler has no `HirObjectProp` for them and drops them, so
                // the value would read back as `null`. `check_object_literal`
                // rejects them outright; typing them here would only invent a
                // member the runtime cannot produce.
                varn_core::ast::ObjectProp::Getter { .. }
                | varn_core::ast::ObjectProp::Setter { .. } => {}
                varn_core::ast::ObjectProp::Spread { argument, .. } => {
                    let spread_ty = self.infer_type(argument, bind);
                    if let varn_core::TypeKind::Object(spread_members) = &spread_ty.0 {
                        for m in spread_members {
                            members.push(m.clone());
                        }
                    } else if let varn_core::TypeKind::Named(name, origin) = &spread_ty.0 {
                        let view = crate::binder::BindView::new(bind, self.resolver);
                        if let Some(cms) = view.get_class_members(name.as_ref(), origin.as_deref()) {
                            for cm in cms {
                                if !cm.is_static {
                                    members.push(ObjectTypeMember::Property {
                                        name: cm.name.clone(),
                                        ty: cm.ty.clone(),
                                        optional: cm.is_optional,
                                        readonly: cm.is_readonly,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
        Type::object(members)
    }

    /// The type of a `function (…) {}` expression.
    ///
    /// The checker had no arm for these at all, so they fell through to the
    /// catch-all and every one of them read as `(...args: dynamic[]) => void`:
    /// the declared parameters and return type were thrown away, and calling
    /// one yielded `dynamic`.
    ///
    /// Without a return annotation the answer is `dynamic`, not `void`.
    /// Inferring it from the body would need the function's own scope, and
    /// walking into it here would desync the check pass's child-scope cursor.
    /// `dynamic` says "unknown"; `void` said "returns nothing", which was
    /// simply false.
    pub(crate) fn infer_function_expr_type(
        &mut self,
        params: &[varn_core::ast::Param],
        return_type: &Option<varn_core::ast::TypeNode>,
        is_async: bool,
        is_generator: bool,
        bind: &BindResult,
    ) -> Type {
        let declared = return_type
            .as_ref()
            .map(|rt| self.resolve_type_node_cached(rt, bind));
        let ret = if is_generator {
            crate::types::generator_of(declared.unwrap_or(Type::Dynamic), is_async)
        } else {
            crate::types::async_fn_return(declared.unwrap_or(Type::Dynamic), is_async)
        };
        Type::fn_(crate::types::FunctionType {
            params: self.signature_params(params, bind),
            return_type: Box::new(ret),
            is_arrow: false,
            type_params: Vec::new(),
        })
    }

    fn signature_params(
        &mut self,
        params: &[varn_core::ast::Param],
        bind: &BindResult,
    ) -> Vec<crate::types::FunctionParam> {
        params
            .iter()
            .map(|p| {
                let mut ty = p
                    .type_ann
                    .as_ref()
                    .or(match &p.pattern {
                        varn_core::ast::Pattern::Identifier { type_ann, .. } => type_ann.as_ref(),
                        _ => None,
                    })
                    .map(|ann| self.resolve_type_node_cached(ann, bind))
                    .unwrap_or(Type::Dynamic);
                if p.is_rest && !matches!(ty.0, varn_core::TypeKind::Array(_)) {
                    ty = Type::array(ty);
                }
                crate::types::FunctionParam {
                    name: Some(Rc::from(crate::binder::pattern_lead_name(&p.pattern))),
                    ty,
                    optional: p.is_optional || p.default.is_some(),
                    is_rest: p.is_rest,
                }
            })
            .collect()
    }
}

fn widen_literal(ty: Type) -> Type {
    crate::binder::widen_literal(ty)
}

/// The member name a property key contributes to an object type. Computed keys
/// (`{ [k]: v }`) name no single member, so they contribute none.
fn prop_key_name(key: &varn_core::ast::expr::PropKey) -> Option<Rc<str>> {
    match key {
        varn_core::ast::expr::PropKey::Identifier(n) | varn_core::ast::expr::PropKey::Str(n) => {
            Some(Rc::from(n.as_str()))
        }
        _ => None,
    }
}
