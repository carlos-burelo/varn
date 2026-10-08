use super::Checker;
use crate::binder::BindResult;
use crate::types::{ObjectTypeMember, Type, TypeContext};
use std::sync::Arc;
use varn_core::ast::ExprId;
use varn_core::TypeKind;

impl<'r> Checker<'r> {
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
        let obj_kind = self.ty_table.get(obj_ty.0);
        match obj_kind {
            TypeKind::Array(inner) if prop_ty.is_int() => Type::resolved(inner),
            TypeKind::Primitive(varn_core::LangPrimitive::Str) if prop_ty.is_int() => Type::Str,
            TypeKind::Builtin(varn_core::BuiltinType::Bytes) if prop_ty.is_int() => Type::Int,
            TypeKind::Named(name, _)
                if prop_ty.is_int()
                    && bind.interner.get(varn_core::LangPrimitive::Str.name()) == Some(name) =>
            {
                Type::Str
            }
            TypeKind::Named(name, _)
                if bind.interner.get(varn_core::BuiltinType::Bytes.name()) == Some(name)
                    && prop_ty.is_int() =>
            {
                Type::Int
            }
            TypeKind::Generic(name, args, _)
                if bind.interner.get(varn_core::BuiltinType::Map.name()) == Some(name) =>
            {
                let arg_ids = self.ty_table.get_list(args).to_vec();
                if arg_ids.len() == 2 {
                    Type::resolved(arg_ids[1])
                } else {
                    Type::Dynamic
                }
            }
            TypeKind::Builtin(varn_core::BuiltinType::Map) => Type::Dynamic,
            TypeKind::Object(mid) => self
                .ty_table
                .get_object_members(mid)
                .iter()
                .find_map(|m| match m {
                    ObjectTypeMember::Index { value_ty, .. } => Some(Type::resolved(*value_ty)),
                    ObjectTypeMember::Property { .. }
                    | ObjectTypeMember::Method { .. }
                    | ObjectTypeMember::Callable { .. } => None,
                })
                .unwrap_or(Type::Dynamic),
            TypeKind::Primitive(_)
            | TypeKind::Builtin(_)
            | TypeKind::Literal(_)
            | TypeKind::This
            | TypeKind::Array(_)
            | TypeKind::Union(_)
            | TypeKind::Intersection(_)
            | TypeKind::Tuple(_)
            | TypeKind::Named(..)
            | TypeKind::Generic(..)
            | TypeKind::TemplateLiteral(_)
            | TypeKind::Fn(_)
            | TypeKind::Typeof(_)
            | TypeKind::KeyOf(_)
            | TypeKind::IndexedAccess { .. }
            | TypeKind::Mapped { .. }
            | TypeKind::Conditional { .. }
            | TypeKind::Infer(_)
            | TypeKind::EnumVariant { .. }
            | TypeKind::TypePredicate { .. } => Type::Dynamic,
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
                let exp =
                    expected.non_nullified(&mut *std::sync::Arc::make_mut(&mut self.ty_table));
                let exp_kind = self.ty_table.get(exp.0);
                let is_index_signature = match exp_kind {
                    TypeKind::Object(mid) => {
                        let members = self.ty_table.get_object_members(mid);
                        members.len() == 1 && matches!(&members[0], ObjectTypeMember::Index { .. })
                    }
                    TypeKind::Primitive(_)
                    | TypeKind::Builtin(_)
                    | TypeKind::Literal(_)
                    | TypeKind::This
                    | TypeKind::Array(_)
                    | TypeKind::Union(_)
                    | TypeKind::Intersection(_)
                    | TypeKind::Tuple(_)
                    | TypeKind::Named(..)
                    | TypeKind::Generic(..)
                    | TypeKind::TemplateLiteral(_)
                    | TypeKind::Fn(_)
                    | TypeKind::Typeof(_)
                    | TypeKind::KeyOf(_)
                    | TypeKind::IndexedAccess { .. }
                    | TypeKind::Mapped { .. }
                    | TypeKind::Conditional { .. }
                    | TypeKind::Infer(_)
                    | TypeKind::EnumVariant { .. }
                    | TypeKind::TypePredicate { .. } => false,
                };
                if is_index_signature {
                    for prop in properties {
                        if let varn_core::ast::ObjectProp::Property { value, .. } = prop {
                            self.infer_type(*value, bind);
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
                        &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                        &bind.interner,
                    );
                    members.push(ObjectTypeMember::Method {
                        name,
                        params: self.signature_params(params, bind),
                        return_type: ret.0,
                        optional: false,
                        is_arrow: false,
                    });
                }
                varn_core::ast::ObjectProp::Getter { .. }
                | varn_core::ast::ObjectProp::Setter { .. } => {}
                varn_core::ast::ObjectProp::Spread { argument, .. } => {
                    let spread_ty = self.infer_type(*argument, bind);
                    let spread_kind = self.ty_table.get(spread_ty.0);
                    if let varn_core::TypeKind::Object(mid) = spread_kind {
                        for m in self.ty_table.get_object_members(mid).to_vec() {
                            members.push(m);
                        }
                    } else if let varn_core::TypeKind::Named(name, origin) = spread_kind {
                        let name_str = bind.interner.resolve(name).to_string();
                        let origin_str = origin.map(|o| bind.interner.resolve(o).to_string());
                        let view = crate::binder::BindView::new(bind, self.resolver);
                        if let Some(cms) = view.get_class_members(&name_str, origin_str.as_deref())
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
        Type::object(members, &mut *std::sync::Arc::make_mut(&mut self.ty_table))
    }

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
                &mut *std::sync::Arc::make_mut(&mut self.ty_table),
            )
        } else {
            crate::types::async_fn_return(
                declared.unwrap_or(Type::Dynamic),
                is_async,
                &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                &bind.interner,
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
            &mut *std::sync::Arc::make_mut(&mut self.ty_table),
        )
    }

    pub(super) fn signature_params(
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
                    .map(|ann| self.resolve_type_node_cached(ann, bind))
                    .unwrap_or(Type::Dynamic);
                if p.is_rest {
                    let is_array = matches!(self.ty_table.get(ty.0), varn_core::TypeKind::Array(_));
                    if !is_array {
                        ty = Type::array(ty, &mut *std::sync::Arc::make_mut(&mut self.ty_table));
                    }
                }
                crate::types::FunctionParam {
                    name: Some(Arc::from(crate::binder::pattern_lead_name(
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

pub(super) fn prop_key_name(key: &varn_core::ast::expr::PropKey) -> Option<Arc<str>> {
    match key {
        varn_core::ast::expr::PropKey::Identifier(n) | varn_core::ast::expr::PropKey::Str(n) => {
            Some(Arc::from(n.as_str()))
        }
        varn_core::ast::expr::PropKey::Int(_) | varn_core::ast::expr::PropKey::Computed(_) => None,
    }
}
