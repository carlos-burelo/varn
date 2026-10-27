use super::Checker;
use crate::checker::recorder::Recorder;
use std::sync::Arc;
use varn_core::ast::ExprId;
use varn_core::TypeKind;
use varn_sem::bind::BindResult;
use varn_sem::types::{ObjectTypeMember, Type, TypeContext};

impl Checker<'_> {
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
                TypeKind::Object(mid) | TypeKind::Record(mid) => {
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
        let members = self.infer_object_members(rec, properties, bind);
        Type::object(members, Arc::make_mut(&mut self.ty_table))
    }

    pub(super) fn infer_record(
        &mut self,
        rec: &mut Recorder,
        properties: &[varn_core::ast::ObjectProp],
        bind: &BindResult,
    ) -> Type {
        let members = self.infer_object_members(rec, properties, bind);
        Type::record(members, Arc::make_mut(&mut self.ty_table))
    }

    fn infer_object_members(
        &mut self,
        rec: &mut Recorder,
        properties: &[varn_core::ast::ObjectProp],
        bind: &BindResult,
    ) -> Vec<ObjectTypeMember> {
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
                    if let varn_core::TypeKind::Object(mid) | varn_core::TypeKind::Record(mid) =
                        spread_kind
                    {
                        for mut m in self.ty_table.get_object_members(mid).to_vec() {
                            if let ObjectTypeMember::Property { readonly, .. } = &mut m {
                                *readonly = false;
                            }
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
        members
    }
}

pub(super) fn prop_key_name(key: &varn_core::ast::expr::PropKey) -> Option<Arc<str>> {
    match key {
        varn_core::ast::expr::PropKey::Identifier(n) | varn_core::ast::expr::PropKey::Str(n) => {
            Some(Arc::from(n.as_str()))
        }
        varn_core::ast::expr::PropKey::Int(n) => Some(Arc::from(n.to_string())),
        varn_core::ast::expr::PropKey::Computed(_) => None,
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
