use super::Checker;
use crate::checker::recorder::Recorder;
use std::sync::Arc;
use varn_core::ast::ExprId;
use varn_core::TypeKind;
use varn_sem::bind::BindResult;
use varn_sem::types::{ObjectTypeMember, Type, TypeContext};

impl<'r> Checker<'r> {
    pub(crate) fn infer_computed_member(
        &mut self,
        rec: &mut Recorder,
        object: ExprId,
        property: ExprId,
        _expr: ExprId,
        bind: &BindResult,
    ) -> Type {
        let (slot, keyed) = self.computed_member_slot(rec, object, property, bind);
        if keyed && !slot.is_dynamic() {
            Type::make_nullable(slot, &mut *std::sync::Arc::make_mut(&mut self.ty_table))
        } else {
            slot
        }
    }

    pub(crate) fn computed_member_slot(
        &mut self,
        rec: &mut Recorder,
        object: ExprId,
        property: ExprId,
        bind: &BindResult,
    ) -> (Type, bool) {
        let arena = self.ast_arena;
        let obj_ty = self.infer_type(rec, object, bind);
        if matches!(
            arena.expr(property).kind,
            varn_core::ast::ExprKind::Range { .. }
        ) {
            return (obj_ty, false);
        }
        let prop_ty = self.infer_type(rec, property, bind);
        let obj_kind = self.ty_table.get(obj_ty.0);
        let builtin = obj_ty.builtin_of(&self.ty_table);
        let keyed = match obj_kind {
            TypeKind::Generic(..) | TypeKind::Named(..) => {
                builtin == Some(varn_core::BuiltinType::Map)
            }
            TypeKind::Object(mid) => self
                .ty_table
                .get_object_members(mid)
                .iter()
                .any(|m| matches!(m, ObjectTypeMember::Index { .. })),
            TypeKind::Primitive(_)
            | TypeKind::Literal(_)
            | TypeKind::This
            | TypeKind::Array(_)
            | TypeKind::Union(_)
            | TypeKind::Intersection(_)
            | TypeKind::Tuple(_)
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
        let slot = match obj_kind {
            TypeKind::Array(inner) if prop_ty.is_int() => Type::resolved(inner),
            TypeKind::Tuple(slots) if prop_ty.is_int() => {
                let slots: Vec<Type> = self
                    .ty_table
                    .get_list(slots)
                    .iter()
                    .map(|slot| Type::resolved(*slot))
                    .collect();
                if let varn_core::ast::ExprKind::IntLiteral { value, .. } =
                    &arena.expr(property).kind
                {
                    return (
                        usize::try_from(*value)
                            .ok()
                            .and_then(|at| slots.get(at).copied())
                            .unwrap_or(Type::Dynamic),
                        false,
                    );
                }
                match slots.split_first() {
                    Some((first, rest)) if rest.iter().all(|slot| slot == first) => *first,
                    Some(_) => {
                        Type::union(slots, &mut *std::sync::Arc::make_mut(&mut self.ty_table))
                    }
                    None => Type::Dynamic,
                }
            }
            TypeKind::Primitive(varn_core::LangPrimitive::Str) if prop_ty.is_int() => Type::Str,
            TypeKind::Named(..)
                if builtin == Some(varn_core::BuiltinType::Bytes) && prop_ty.is_int() =>
            {
                Type::Int
            }
            TypeKind::Generic(..) | TypeKind::Named(..)
                if builtin == Some(varn_core::BuiltinType::Map) =>
            {
                obj_ty.builtin_arg(1, &self.ty_table)
            }
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
        };
        (slot, keyed)
    }

    pub(crate) fn infer_object_type(
        &mut self,
        rec: &mut Recorder,
        properties: &[varn_core::ast::ObjectProp],
        bind: &BindResult,
        _expr: ExprId,
    ) -> Type {
        let has_methods = properties
            .iter()
            .any(|p| matches!(p, varn_core::ast::ObjectProp::Method { .. }));
        if !has_methods && let Some(expected) = self.expected_type {
            let exp = expected.non_nullified(&mut *std::sync::Arc::make_mut(&mut self.ty_table));
            let exp_kind = self.ty_table.get(exp.0);
            let is_index_signature = match exp_kind {
                TypeKind::Object(mid) => {
                    let members = self.ty_table.get_object_members(mid);
                    members.len() == 1 && matches!(&members[0], ObjectTypeMember::Index { .. })
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
                        self.infer_type(rec, *value, bind);
                    }
                }
                return exp;
            }
        }
        let mut members = Vec::new();
        for prop in properties {
            match prop {
                varn_core::ast::ObjectProp::Property { key, value, .. } => {
                    let Some(name) = prop_key_name(key) else {
                        continue;
                    };
                    let ty = self.infer_type(rec, *value, bind);
                    put_member(
                        &mut members,
                        ObjectTypeMember::Property {
                            name,
                            ty: ty.0,
                            optional: false,
                            readonly: false,
                        },
                    );
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
                    let ret = varn_sem::types::async_fn_return(
                        ret,
                        *is_async,
                        &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                    );
                    let params = self.signature_params(params, bind);
                    put_member(
                        &mut members,
                        ObjectTypeMember::Method {
                            name,
                            params,
                            return_type: ret.0,
                            optional: false,
                            is_arrow: false,
                            type_params: Vec::new(),
                        },
                    );
                }
                varn_core::ast::ObjectProp::Spread { argument, .. } => {
                    let spread_ty = self.infer_type(rec, *argument, bind);
                    let spread_kind = self.ty_table.get(spread_ty.0);
                    if let varn_core::TypeKind::Object(mid) = spread_kind {
                        for m in self.ty_table.get_object_members(mid).to_vec() {
                            put_member(&mut members, m);
                        }
                    } else if let varn_core::TypeKind::Named(name, origin) = spread_kind {
                        let name_str = bind.interner.resolve(name).to_string();
                        let origin_str = origin.map(|o| bind.interner.resolve(o).to_string());
                        let view = varn_sem::bind::BindView::new(bind, self.resolver);
                        if let Some(cms) = view.get_class_members(&name_str, origin_str.as_deref())
                        {
                            for cm in cms {
                                if !cm.is_static {
                                    put_member(
                                        &mut members,
                                        ObjectTypeMember::Property {
                                            name: cm.name.clone(),
                                            ty: cm.ty.0,
                                            optional: cm.is_optional,
                                            readonly: cm.is_readonly,
                                        },
                                    );
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
            varn_sem::types::declared_generator(
                declared.unwrap_or(Type::Dynamic),
                is_async,
                &mut *std::sync::Arc::make_mut(&mut self.ty_table),
            )
        } else {
            varn_sem::types::async_fn_return(
                declared.unwrap_or(Type::Dynamic),
                is_async,
                &mut *std::sync::Arc::make_mut(&mut self.ty_table),
            )
        };
        let params = self.signature_params(params, bind);
        Type::fn_(
            varn_sem::types::FunctionType {
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
    ) -> Vec<varn_sem::types::FunctionParam> {
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
                varn_sem::types::FunctionParam {
                    name: Some(Arc::from(varn_sem::infer::pattern_lead_name(
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

fn member_name(member: &ObjectTypeMember) -> Option<&Arc<str>> {
    match member {
        ObjectTypeMember::Property { name, .. } | ObjectTypeMember::Method { name, .. } => {
            Some(name)
        }
        ObjectTypeMember::Index { .. } | ObjectTypeMember::Callable { .. } => None,
    }
}

fn put_member(members: &mut Vec<ObjectTypeMember>, member: ObjectTypeMember) {
    let existing = member_name(&member)
        .and_then(|name| members.iter().position(|m| member_name(m) == Some(name)));
    match existing {
        Some(at) => members[at] = member,
        None => members.push(member),
    }
}
