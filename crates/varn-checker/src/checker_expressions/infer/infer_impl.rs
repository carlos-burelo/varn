use crate::binder::BindResult;
use crate::checker::Checker;
use crate::types::TypeContext;
use crate::types::{CheckerTyId, ObjectTypeMember, Type};
use std::rc::Rc;
use varn_core::ast::{AstArena, ExprId, ExprKind};
use varn_core::{Diagnostic, ErrorCode, IntrinsicType, TypeKind, TypeTag};

use super::member_binary::{infer_binary_type, infer_member_type};

impl<'r> Checker<'r> {
    pub(super) fn infer_type_impl(&mut self, expr: ExprId, bind: &BindResult) -> Type {
        let arena = self.ast_arena;
        match &arena.expr(expr).kind {
            ExprKind::Identifier { name } => {
                let name_str = bind.interner.resolve(*name);
                // Pipeline placeholder `_` stands for the piped value, so it
                // carries that value's type (`x |> f(_, y)` ⇒ `_` has `x`'s type).
                if name_str == "_" && self.in_pipeline_rhs {
                    return self.pipeline_value_type.clone().unwrap_or(Type::Dynamic);
                }
                let scope = bind.scopes.get(self.current_scope);
                if let Some(sid) = scope.resolve(*name, &bind.scopes) {
                    if let Some(ty) = self.symbol_types.get(&sid) {
                        return ty.clone();
                    }
                    if let Some(ty) = bind.arena.get(sid).ty.clone() {
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
                .map(|cn| match IntrinsicType::from_str(cn) {
                    Some(it) if it.is_scalar_primitive() => {
                        Type::intrinsic(it.0, &mut self.ty_table)
                    }
                    _ => Type::named(cn.to_string(), self.resolver, &mut self.ty_table),
                })
                .unwrap_or(Type::Dynamic),

            ExprKind::Super => self
                .current_class
                .as_ref()
                .and_then(|cn| bind.class_parents.get(cn))
                .map(|parent| Type::named(parent.clone(), self.resolver, &mut self.ty_table))
                .unwrap_or(Type::Dynamic),
            ExprKind::New {
                callee, type_args, ..
            } => {
                let callee = *callee;
                let callee_ty = self.infer_type(callee, bind);
                if callee_ty.is_dynamic() {
                    return Type::Dynamic;
                }
                let callee_kind = *self.ty_table.get(callee_ty.0);
                match callee_kind {
                    TypeKind::Named(name, origin) => {
                        let name_str = bind.interner.resolve(name).to_string();
                        let origin_str =
                            origin.map(|o| Rc::from(bind.interner.resolve(o)));
                        if !type_args.is_empty() {
                            let args: Vec<Type> = type_args
                                .iter()
                                .map(|a| self.resolve_type_node_cached(a, bind))
                                .collect();
                            Type::generic_with_origin(
                                name_str,
                                args,
                                origin_str,
                                self.resolver,
                                &mut self.ty_table,
                            )
                        } else if name_str == IntrinsicType::Map.as_str() {
                            Type::generic_with_origin(
                                name_str,
                                vec![Type::Dynamic],
                                origin_str,
                                self.resolver,
                                &mut self.ty_table,
                            )
                        } else {
                            Type::named_with_origin(
                                name_str,
                                origin_str,
                                self.resolver,
                                &mut self.ty_table,
                            )
                        }
                    }
                    TypeKind::Generic(name, args, origin) => {
                        let name_str = bind.interner.resolve(name).to_string();
                        let origin_str =
                            origin.map(|o| Rc::from(bind.interner.resolve(o)));
                        let arg_ids = self.ty_table.get_list(args).to_vec();
                        let args: Vec<Type> =
                            arg_ids.into_iter().map(|id| Type(id, false)).collect();
                        Type::generic_with_origin(
                            name_str,
                            args,
                            origin_str,
                            self.resolver,
                            &mut self.ty_table,
                        )
                    }
                    _ => {
                        if let ExprKind::Identifier { name } = &arena.expr(callee).kind {
                            let name_str = bind.interner.resolve(*name).to_string();
                            if !type_args.is_empty() {
                                let args: Vec<Type> = type_args
                                    .iter()
                                    .map(|a| self.resolve_type_node_cached(a, bind))
                                    .collect();
                                Type::generic(name_str, args, self.resolver, &mut self.ty_table)
                            } else if name_str == IntrinsicType::Map.as_str() {
                                Type::generic(
                                    name_str,
                                    vec![Type::Dynamic],
                                    self.resolver,
                                    &mut self.ty_table,
                                )
                            } else {
                                Type::named(name_str, self.resolver, &mut self.ty_table)
                            }
                        } else {
                            Type::Dynamic
                        }
                    }
                }
            }
            ExprKind::Call { .. } => self.infer_call_type(expr, bind),
            ExprKind::TaggedTemplate { tag, .. } => {
                let tag_ty = self.infer_type(*tag, bind);
                let tag_ty = tag_ty.non_nullified(&mut self.ty_table);
                if let TypeKind::Fn(fid) = self.ty_table.get(tag_ty.0) {
                    let ret = self.ty_table.get_function(*fid).return_type;
                    Type(ret, false)
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
                    Type::union(vec![t_ty, f_ty], &mut self.ty_table)
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
                let (params, return_type, body, is_async) =
                    (params.clone(), return_type.clone(), (**body).clone(), *is_async);
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
            ExprKind::Tuple { elements } => {
                let elements = elements.clone();
                let elem_tys: Vec<Type> = elements
                    .iter()
                    .map(|e| self.infer_type(*e, bind))
                    .collect();
                let ids: Vec<CheckerTyId> = elem_tys.iter().map(|t| t.0).collect();
                let list = self.ty_table.intern_list(&ids);
                Type(self.ty_table.intern(TypeKind::Tuple(list)), false)
            }
            ExprKind::Record { properties } => {
                let mut members = Vec::new();
                for prop in properties.clone() {
                    if let varn_core::ast::ObjectProp::Property { key, value, .. } = prop {
                        let ty = self.infer_type(value, bind);
                        let name: std::rc::Rc<str> = match &key {
                            varn_core::ast::PropKey::Identifier(s)
                            | varn_core::ast::PropKey::Str(s) => std::rc::Rc::from(s.as_str()),
                            varn_core::ast::PropKey::Int(n) => {
                                std::rc::Rc::from(n.to_string().as_str())
                            }
                            varn_core::ast::PropKey::Computed(_) => std::rc::Rc::from("<computed>"),
                        };
                        members.push(crate::types::ObjectTypeMember::Property {
                            name,
                            ty: ty.0,
                            optional: false,
                            readonly: true,
                        });
                    }
                }
                Type::object(members, &mut self.ty_table)
            }
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
                let (target, property) = (*target, *property);
                let _target_ty = self.infer_type(target, bind);
                match varn_core::MemberKey::from_str(bind.interner.resolve(property)) {
                    Some(varn_core::MemberKey::Name) | Some(varn_core::MemberKey::Type) => {
                        Type::Str
                    }
                    Some(varn_core::MemberKey::Class) => Type::Dynamic,
                    Some(varn_core::MemberKey::Fields) | Some(varn_core::MemberKey::Methods) => {
                        Type::array(Type::Str, &mut self.ty_table)
                    }
                    Some(varn_core::MemberKey::Keys) => {
                        let ret = Type::array(Type::Str, &mut self.ty_table);
                        Type::fn_(
                            crate::types::FunctionType {
                                params: vec![],
                                return_type: ret.0,
                                is_arrow: true,
                                type_params: vec![],
                            },
                            &mut self.ty_table,
                        )
                    }
                    Some(varn_core::MemberKey::Values) => {
                        let ret = Type::array(Type::Dynamic, &mut self.ty_table);
                        Type::fn_(
                            crate::types::FunctionType {
                                params: vec![],
                                return_type: ret.0,
                                is_arrow: true,
                                type_params: vec![],
                            },
                            &mut self.ty_table,
                        )
                    }
                    Some(varn_core::MemberKey::Entries) => {
                        let ids: Vec<CheckerTyId> = vec![Type::Str.0, Type::Dynamic.0];
                        let list = self.ty_table.intern_list(&ids);
                        let entry = Type(self.ty_table.intern(TypeKind::Tuple(list)), false);
                        let ret = Type::array(entry, &mut self.ty_table);
                        Type::fn_(
                            crate::types::FunctionType {
                                params: vec![],
                                return_type: ret.0,
                                is_arrow: true,
                                type_params: vec![],
                            },
                            &mut self.ty_table,
                        )
                    }
                    Some(varn_core::MemberKey::HasOwn) => Type::fn_(
                        crate::types::FunctionType {
                            params: vec![crate::types::FunctionParam {
                                name: Some(std::rc::Rc::from("key")),
                                ty: Type::Str.0,
                                optional: false,
                                is_rest: false,
                            }],
                            return_type: Type::Bool.0,
                            is_arrow: true,
                            type_params: vec![],
                        },
                        &mut self.ty_table,
                    ),
                    _ => Type::Dynamic,
                }
            }
            ExprKind::Await { argument } => {
                let inner = self.infer_type(*argument, bind);
                crate::types::awaited(&inner, &self.ty_table, &bind.interner)
            }
            ExprKind::NonNull { expression } => {
                let ty = self.infer_type(*expression, bind);
                if let TypeKind::Union(list) = *self.ty_table.get(ty.0) {
                    let ids = self.ty_table.get_list(list).to_vec();
                    let filtered: Vec<Type> = ids
                        .into_iter()
                        .filter(|id| {
                            !matches!(
                                self.ty_table.get(*id),
                                TypeKind::Intrinsic(TypeTag::Null)
                                    | TypeKind::Intrinsic(TypeTag::Void)
                            )
                        })
                        .map(|id| Type(id, false))
                        .collect();
                    if filtered.len() == 1 {
                        return filtered[0];
                    }
                    return Type::union(filtered, &mut self.ty_table);
                }
                ty
            }
            ExprKind::Try { expression } => {
                let ty = self.infer_type(*expression, bind);
                match *self.ty_table.get(ty.0) {
                    TypeKind::Generic(name, args_list, _)
                        if (bind.interner.resolve(name) == "Result"
                            || bind.interner.resolve(name) == "Option") =>
                    {
                        let ids = self.ty_table.get_list(args_list).to_vec();
                        match ids.first() {
                            Some(first) => Type(*first, false),
                            None => Type::Dynamic,
                        }
                    }
                    _ if ty.is_nullable(&self.ty_table) => ty.non_nullified(&mut self.ty_table),
                    _ => Type::Dynamic,
                }
            }
            ExprKind::Logical { op, left, right } => {
                let (op, left, right) = (*op, *left, *right);
                let l_ty = self.infer_type(left, bind);
                let r_ty = self.infer_type(right, bind);
                match op {
                    varn_core::ast::LogicalOp::And => {
                        if l_ty == r_ty {
                            l_ty
                        } else {
                            Type::union(vec![l_ty, r_ty], &mut self.ty_table)
                        }
                    }
                    varn_core::ast::LogicalOp::Nullish => {
                        let l_non_null = l_ty.non_nullified(&mut self.ty_table);
                        if l_non_null == r_ty {
                            r_ty
                        } else {
                            Type::union(vec![l_non_null, r_ty], &mut self.ty_table)
                        }
                    }
                    varn_core::ast::LogicalOp::Or => {
                        if l_ty == r_ty {
                            l_ty
                        } else {
                            Type::union(vec![l_ty, r_ty], &mut self.ty_table)
                        }
                    }
                }
            }
            ExprKind::Binary { op, left, right } => {
                let (op, left, right) = (*op, *left, *right);
                infer_binary_type(self, op, left, right, bind)
            }
            ExprKind::Unary { op, operand, .. } => {
                let (op, operand) = (*op, *operand);
                match op {
                    varn_core::ast::operators::UnaryOp::Not => Type::Bool,
                    varn_core::ast::operators::UnaryOp::Minus
                    | varn_core::ast::operators::UnaryOp::Plus => self.infer_type(operand, bind),
                    varn_core::ast::operators::UnaryOp::Typeof => Type::Str,
                    varn_core::ast::operators::UnaryOp::BitNot => {
                        let inner = self.infer_type(operand, bind);
                        if inner.is_int() {
                            Type::intrinsic(TypeTag::Int, &mut self.ty_table)
                        } else {
                            Type::Dynamic
                        }
                    }
                }
            }
            ExprKind::Update { operand, .. } => self.infer_type(*operand, bind),
            ExprKind::Assign { value, .. } => self.infer_type(*value, bind),
            ExprKind::Array { elements } => {
                let elements = elements.clone();
                let mut elem_tys = Vec::new();
                for el in &elements {
                    match el {
                        varn_core::ast::ArrayEl::Expr(e) => {
                            let ty = self.infer_type(*e, bind);
                            if !ty.is_dynamic() {
                                elem_tys.push(ty);
                            }
                        }
                        varn_core::ast::ArrayEl::Spread(e) => {
                            let ty = self.infer_type(*e, bind);
                            if let TypeKind::Array(inner) = *self.ty_table.get(ty.0) {
                                elem_tys.push(Type(inner, false));
                            }
                        }
                        _ => {}
                    }
                }
                if elem_tys.is_empty() {
                    if let Some(expected) = self.expected_type {
                        let non_null = expected.non_nullified(&mut self.ty_table);
                        if let TypeKind::Array(inner) = *self.ty_table.get(non_null.0) {
                            Type::array(Type(inner, false), &mut self.ty_table)
                        } else {
                            Type::array(Type::Dynamic, &mut self.ty_table)
                        }
                    } else {
                        Type::array(Type::Dynamic, &mut self.ty_table)
                    }
                } else {
                    let first = elem_tys[0];
                    if elem_tys.iter().all(|t| t == &first) {
                        let widened = widen_literal(first);
                        Type::array(widened, &mut self.ty_table)
                    } else {
                        let unioned = Type::union(elem_tys, &mut self.ty_table);
                        let widened = widen_literal(unioned);
                        Type::array(widened, &mut self.ty_table)
                    }
                }
            }
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
            ExprKind::Range { .. } => Type::intrinsic(varn_core::TypeTag::Range, &mut self.ty_table),
            ExprKind::Match { cases, .. } => {
                let cases = cases.clone();
                let mut tys = Vec::new();
                for case in &cases {
                    match &case.body {
                        varn_core::ast::MatchBody::Expr(e) => {
                            let ty = self.infer_type(*e, bind);
                            tys.push(ty);
                        }
                        varn_core::ast::MatchBody::Block(stmt) => {
                            if stmt_terminates(*stmt, arena) {
                                tys.push(Type::Never);
                            } else {
                                tys.push(Type::Void);
                            }
                        }
                    }
                }
                let non_never: Vec<Type> = tys.iter().filter(|t| !t.is_never()).cloned().collect();
                if non_never.is_empty() {
                    if tys.is_empty() {
                        Type::Dynamic
                    } else {
                        Type::Never
                    }
                } else if non_never.len() == 1 {
                    non_never.into_iter().next().unwrap()
                } else {
                    Type::union(non_never, &mut self.ty_table)
                }
            }
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
                    TypeKind::Fn(fid) => Type(self.ty_table.get_function(*fid).return_type, false),
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
        object: ExprId,
        property: ExprId,
        _expr: ExprId,
        bind: &BindResult,
    ) -> Type {
        let arena = self.ast_arena;
        let obj_ty = self.infer_type(object, bind);
        if matches!(
            arena.expr(property).kind,
            varn_core::ast::ExprKind::Range { .. }
        ) {
            return obj_ty;
        }
        let prop_ty = self.infer_type(property, bind);
        let obj_kind = *self.ty_table.get(obj_ty.0);
        match obj_kind {
            TypeKind::Array(inner) if prop_ty.is_int() => Type(inner, false),
            TypeKind::Intrinsic(TypeTag::Str) if prop_ty.is_int() => Type::Str,
            TypeKind::Intrinsic(TypeTag::Bytes) if prop_ty.is_int() => Type::Int,
            TypeKind::Named(name, _)
                if prop_ty.is_int()
                    && bind.interner.get(IntrinsicType::Str.as_str()) == Some(name) =>
            {
                Type::Str
            }
            TypeKind::Named(name, _)
                if bind.interner.get(IntrinsicType::Bytes.as_str()) == Some(name) && prop_ty.is_int() =>
            {
                Type::Int
            }
            TypeKind::Generic(name, args, _)
                if bind.interner.get(IntrinsicType::Map.as_str()) == Some(name) =>
            {
                let arg_ids = self.ty_table.get_list(args).to_vec();
                if arg_ids.len() == 2 {
                    Type(arg_ids[1], false)
                } else if arg_ids.len() == 1 {
                    Type(arg_ids[0], false)
                } else {
                    Type::Dynamic
                }
            }
            TypeKind::Intrinsic(TypeTag::Map) => Type::Dynamic,
            TypeKind::Object(mid) => self
                .ty_table
                .get_object_members(mid)
                .iter()
                .find_map(|m| match m {
                    ObjectTypeMember::Index { value_ty, .. } => Some(Type(*value_ty, false)),
                    _ => None,
                })
                .unwrap_or(Type::Dynamic),
            _ => Type::Dynamic,
        }
    }

    pub(crate) fn infer_object_type(
        &mut self,
        properties: &[varn_core::ast::ObjectProp],
        bind: &BindResult,
        _expr: ExprId,
    ) -> Type {
        let has_methods = properties.iter().any(|p| {
            matches!(
                p,
                varn_core::ast::ObjectProp::Method { .. }
                    | varn_core::ast::ObjectProp::Getter { .. }
                    | varn_core::ast::ObjectProp::Setter { .. }
            )
        });
        if !has_methods {
            if let Some(expected) = self.expected_type {
                let exp = expected.non_nullified(&mut self.ty_table);
                let exp_kind = *self.ty_table.get(exp.0);
                let is_map = match exp_kind {
                    TypeKind::Generic(name, args, _) => {
                        bind.interner.get(IntrinsicType::Map.as_str()) == Some(name)
                            && {
                                let n = self.ty_table.get_list(args).len();
                                n == 1 || n == 2
                            }
                    }
                    TypeKind::Intrinsic(TypeTag::Map) => true,
                    TypeKind::Object(mid) => {
                        let members = self.ty_table.get_object_members(mid);
                        members.len() == 1 && matches!(&members[0], ObjectTypeMember::Index { .. })
                    }
                    _ => false,
                };
                if is_map {
                    for prop in properties {
                        if let varn_core::ast::ObjectProp::Property { value, .. } = prop {
                            self.infer_type(*value, bind);
                        }
                    }
                    if let TypeKind::Object(mid) = exp_kind {
                        let members = self.ty_table.get_object_members(mid).to_vec();
                        if let Some(ObjectTypeMember::Index {
                            key_ty, value_ty, ..
                        }) = members.first()
                        {
                            let map_atom = self.resolver.intern(IntrinsicType::Map.as_str());
                            return Type::generic_atom(
                                map_atom,
                                vec![Type(*key_ty, false), Type(*value_ty, false)],
                                None,
                                &mut self.ty_table,
                            );
                        }
                    }
                    return exp;
                }
            }
        }
        let mut members = Vec::new();
        for prop in properties {
            match prop {
                varn_core::ast::ObjectProp::Property { key, value, .. } => {
                    let Some(name) = prop_key_name(key) else {
                        continue;
                    };
                    let ty = self.infer_type(*value, bind);
                    members.push(ObjectTypeMember::Property {
                        name,
                        ty: ty.0,
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
                    let ret = crate::types::async_fn_return(
                        ret,
                        *is_async,
                        &mut self.ty_table,
                        &bind.interner,
                        Some(self.resolver),
                    );
                    members.push(ObjectTypeMember::Method {
                        name,
                        params: self.signature_params(params, bind),
                        return_type: ret.0,
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
                    let spread_ty = self.infer_type(*argument, bind);
                    let spread_kind = *self.ty_table.get(spread_ty.0);
                    if let varn_core::TypeKind::Object(mid) = spread_kind {
                        for m in self.ty_table.get_object_members(mid).to_vec() {
                            members.push(m);
                        }
                    } else if let varn_core::TypeKind::Named(name, origin) = spread_kind {
                        let name_str = bind.interner.resolve(name).to_string();
                        let origin_str = origin.map(|o| bind.interner.resolve(o).to_string());
                        let view = crate::binder::BindView::new(bind, self.resolver);
                        if let Some(cms) =
                            view.get_class_members(&name_str, origin_str.as_deref())
                        {
                            for cm in cms {
                                if !cm.is_static {
                                    members.push(ObjectTypeMember::Property {
                                        name: cm.name.clone(),
                                        ty: cm.ty.0,
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
        Type::object(members, &mut self.ty_table)
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
            crate::types::generator_of(
                declared.unwrap_or(Type::Dynamic),
                is_async,
                &mut self.ty_table,
                Some(self.resolver),
            )
        } else {
            crate::types::async_fn_return(
                declared.unwrap_or(Type::Dynamic),
                is_async,
                &mut self.ty_table,
                &bind.interner,
                Some(self.resolver),
            )
        };
        let params = self.signature_params(params, bind);
        Type::fn_(
            crate::types::FunctionType {
                params,
                return_type: ret.0,
                is_arrow: false,
                type_params: Vec::new(),
            },
            &mut self.ty_table,
        )
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
                if p.is_rest {
                    let is_array = matches!(self.ty_table.get(ty.0), varn_core::TypeKind::Array(_));
                    if !is_array {
                        ty = Type::array(ty, &mut self.ty_table);
                    }
                }
                crate::types::FunctionParam {
                    name: Some(Rc::from(crate::binder::pattern_lead_name(
                        &p.pattern,
                        &bind.interner,
                    ))),
                    ty: ty.0,
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

fn stmt_terminates(stmt: varn_core::ast::StmtId, arena: &AstArena) -> bool {
    match &arena.stmt(stmt).kind {
        varn_core::ast::StmtKind::Return { .. } | varn_core::ast::StmtKind::Throw { .. } => true,
        varn_core::ast::StmtKind::Block { stmts } => {
            stmts.last().is_some_and(|s| stmt_terminates(*s, arena))
        }
        _ => false,
    }
}
