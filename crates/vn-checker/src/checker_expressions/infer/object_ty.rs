use super::Checker;
use crate::checker::recorder::Recorder;
use std::sync::Arc;
use vn_core::ast::ExprId;
use vn_sem::bind::BindResult;
use vn_sem::types::{ObjectTypeMember, Type, TypeContext};

impl Checker<'_> {
    pub(crate) fn infer_object_type(
        &mut self,
        rec: &mut Recorder,
        properties: &[vn_core::ast::ObjectProp],
        bind: &BindResult,
        expr: ExprId,
    ) -> Type {
        if let Some((map, _, _)) = self.expected_map(expr) {
            return map;
        }
        let (members, duplicates) = self.infer_object_members(rec, properties, bind);
        let range = self.ast_arena.expr(expr).range;
        for name in duplicates {
            self.emit(
                vn_core::Diagnostic::error(
                    vn_core::ErrorCode::DuplicateRecordField,
                    format!("field '{name}' is given more than once: a record field is replaced with `with`, and when two spread records share it the later one wins"),
                )
                .with_range(range),
            );
        }
        Type::record(members, Arc::make_mut(&mut self.ty_table))
    }

    fn infer_object_members(
        &mut self,
        rec: &mut Recorder,
        properties: &[vn_core::ast::ObjectProp],
        bind: &BindResult,
    ) -> (Vec<ObjectTypeMember>, Vec<String>) {
        let mut members = Vec::new();
        let mut duplicates = Vec::new();
        let mut from_spread: Vec<bool> = Vec::new();
        for prop in properties {
            match prop {
                vn_core::ast::ObjectProp::Property { key, value, .. } => {
                    let Some(name) = prop_key_name(key) else {
                        continue;
                    };
                    let ty = self.infer_type(rec, *value, bind);
                    put_member(
                        &mut members,
                        &mut duplicates,
                        &mut from_spread,
                        false,
                        ObjectTypeMember::Property {
                            name,
                            ty: ty.0,
                            optional: false,
                            readonly: false,
                        },
                    );
                }
                vn_core::ast::ObjectProp::Method {
                    key,
                    params,
                    return_type,
                    is_async,
                    body,
                    ..
                } => {
                    let Some(name) = prop_key_name(key) else {
                        continue;
                    };
                    let signature = self.signature_params(params, bind);
                    let ret = match return_type {
                        Some(rt) => self.resolve_type_node_cached(rt, bind),
                        None => self.infer_body_return_type(
                            rec,
                            params,
                            &signature,
                            vn_core::ast::ArrowBody::Block(*body),
                            bind,
                        ),
                    };
                    let ret = vn_sem::types::async_fn_return(
                        ret,
                        *is_async,
                        &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                    );
                    let params = signature;
                    put_member(
                        &mut members,
                        &mut duplicates,
                        &mut from_spread,
                        false,
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
                vn_core::ast::ObjectProp::Spread { argument, .. } => {
                    let spread_ty = self.infer_type(rec, *argument, bind);
                    let spread_kind = self.ty_table.get(spread_ty.0);
                    if let vn_core::TypeKind::Record(mid) | vn_core::TypeKind::Interface(mid) =
                        spread_kind
                    {
                        for mut m in self.ty_table.get_object_members(mid).to_vec() {
                            if let ObjectTypeMember::Property { readonly, .. } = &mut m {
                                *readonly = false;
                            }
                            put_member(&mut members, &mut duplicates, &mut from_spread, true, m);
                        }
                    } else if let vn_core::TypeKind::Named(name, origin) = spread_kind {
                        let name_str = bind.interner.resolve(name).to_string();
                        let origin_str = origin.map(|o| bind.interner.resolve(o).to_string());
                        let view = vn_sem::bind::BindView::new(bind, self.resolver);
                        if let Some(cms) = view.get_class_members(&name_str, origin_str.as_deref())
                        {
                            for cm in cms {
                                if !cm.is_static {
                                    put_member(
                                        &mut members,
                                        &mut duplicates,
                                        &mut from_spread,
                                        true,
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
        (members, duplicates)
    }
}

pub(super) fn prop_key_name(key: &vn_core::ast::expr::PropKey) -> Option<Arc<str>> {
    match key {
        vn_core::ast::expr::PropKey::Identifier(n) | vn_core::ast::expr::PropKey::Str(n) => {
            Some(Arc::from(n.as_str()))
        }
        vn_core::ast::expr::PropKey::Int(n) => Some(Arc::from(n.to_string())),
        vn_core::ast::expr::PropKey::Computed(_) => None,
    }
}

fn member_name(member: &ObjectTypeMember) -> Option<&Arc<str>> {
    match member {
        ObjectTypeMember::Property { name, .. } | ObjectTypeMember::Method { name, .. } => {
            Some(name)
        }
        ObjectTypeMember::Callable { .. } => None,
    }
}

fn put_member(
    members: &mut Vec<ObjectTypeMember>,
    duplicates: &mut Vec<String>,
    from_spread: &mut Vec<bool>,
    spread: bool,
    member: ObjectTypeMember,
) {
    let existing = member_name(&member)
        .and_then(|name| members.iter().position(|m| member_name(m) == Some(name)));
    match existing {
        Some(at) => {
            let name = member_name(&member).map(|n| n.to_string());
            if !(spread && from_spread[at]) {
                duplicates.extend(name);
            }
            members[at] = member;
            from_spread[at] = spread;
        }
        None => {
            members.push(member);
            from_spread.push(spread);
        }
    }
}
