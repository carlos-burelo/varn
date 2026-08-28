use varn_core::ast::{ExprKind, ObjectProp, PropKey};

use super::super::type_inference::infer_expr_type;
use super::super::type_resolution::resolve_type_node;
use crate::binder::{pattern_lead_name, ClassMemberInfo, ClassMemberKind};
use crate::types::Type;
use std::rc::Rc;

impl<'r> super::super::Binder<'r> {
    pub(crate) fn collect_object_members(&self, props: &[ObjectProp]) -> Vec<ClassMemberInfo> {
        use crate::types::FunctionType;
        use varn_core::TypeKind;

        props
            .iter()
            .filter_map(|prop| {
                let range = prop.range();
                match prop {
                    ObjectProp::Property { key, value, .. } => {
                        let name = match key {
                            PropKey::Identifier(s) | PropKey::Str(s) => Rc::from(s.as_str()),
                            _ => return None,
                        };
                        let ty = infer_expr_type(value, Some(self));
                        let nested_members = if let ExprKind::Object { properties } = &value.kind {
                            self.collect_object_members(properties)
                        } else {
                            Vec::new()
                        };

                        let kind = if matches!(&ty.0, TypeKind::Fn(_)) {
                            ClassMemberKind::Method
                        } else {
                            ClassMemberKind::Property
                        };

                        Some(ClassMemberInfo {
                            name,
                            kind,
                            is_async: false,
                            is_generator: false,
                            is_static: false,
                            is_optional: false,
                            line: range.start.line.saturating_sub(1),
                            col: range.start.column,
                            offset: range.start.offset,
                            ty,
                            members: nested_members,
                            visibility: None,
                            is_abstract: false,
                            is_readonly: false,
                            is_override: false,
                            symbol_id: None,
                            ..Default::default()
                        })
                    }
                    ObjectProp::Method {
                        key,
                        params,
                        return_type: ret_ann,
                        ..
                    } => {
                        let name = match key {
                            PropKey::Identifier(s) | PropKey::Str(s) => Rc::from(s.as_str()),
                            _ => return None,
                        };
                        let ret_ty = ret_ann
                            .as_ref()
                            .map(|ann| resolve_type_node(ann, Some(self)))
                            .unwrap_or(Type::Dynamic);

                        let fn_params: Vec<_> = params
                            .iter()
                            .map(|p| crate::types::FunctionParam {
                                name: Some(Rc::from(pattern_lead_name(&p.pattern))),
                                ty: p
                                    .type_ann
                                    .as_ref()
                                    .map(|ann| resolve_type_node(ann, Some(self)))
                                    .unwrap_or(Type::Dynamic),
                                optional: p.is_optional || p.default.is_some(),
                                is_rest: p.is_rest,
                            })
                            .collect();

                        let ty = Type::fn_(FunctionType {
                            params: fn_params,
                            return_type: Box::new(ret_ty.clone()),
                            is_arrow: false,
                            type_params: Vec::new(),
                        });

                        Some(ClassMemberInfo {
                            name,
                            kind: ClassMemberKind::Method,
                            is_async: false,
                            is_generator: false,
                            is_static: false,
                            is_optional: false,
                            line: range.start.line.saturating_sub(1),
                            col: range.start.column,
                            offset: range.start.offset,
                            ty,
                            members: Vec::new(),
                            visibility: None,
                            is_abstract: false,
                            is_readonly: false,
                            is_override: false,
                            symbol_id: None,
                            ..Default::default()
                        })
                    }
                    ObjectProp::Getter { key, .. } => {
                        let name = match key {
                            PropKey::Identifier(s) | PropKey::Str(s) => Rc::from(s.as_str()),
                            _ => return None,
                        };
                        Some(ClassMemberInfo {
                            name,
                            kind: ClassMemberKind::Getter,
                            is_async: false,
                            is_generator: false,
                            is_static: false,
                            is_optional: false,
                            line: range.start.line.saturating_sub(1),
                            col: range.start.column,
                            offset: range.start.offset,
                            ty: Type::Dynamic,
                            members: Vec::new(),
                            visibility: None,
                            is_abstract: false,
                            is_readonly: false,
                            is_override: false,
                            symbol_id: None,
                            ..Default::default()
                        })
                    }
                    ObjectProp::Setter { key, .. } => {
                        let name = match key {
                            PropKey::Identifier(s) | PropKey::Str(s) => Rc::from(s.as_str()),
                            _ => return None,
                        };
                        Some(ClassMemberInfo {
                            name,
                            kind: ClassMemberKind::Setter,
                            is_async: false,
                            is_generator: false,
                            is_static: false,
                            is_optional: false,
                            line: range.start.line.saturating_sub(1),
                            col: range.start.column,
                            offset: range.start.offset,
                            ty: Type::Dynamic,
                            members: Vec::new(),
                            visibility: None,
                            is_abstract: false,
                            is_readonly: false,
                            is_override: false,
                            symbol_id: None,
                            ..Default::default()
                        })
                    }
                    _ => None,
                }
            })
            .collect()
    }
}
