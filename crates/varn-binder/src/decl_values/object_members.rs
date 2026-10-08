use varn_core::ast::{ExprKind, ObjectProp, PropKey};

use std::sync::Arc;
use varn_sem::types::Type;
use varn_sem::types::{ClassMemberInfo, ClassMemberKind};

impl<'r> super::super::Binder<'r> {
    pub(crate) fn collect_object_members(&mut self, props: &[ObjectProp]) -> Vec<ClassMemberInfo> {
        use varn_core::TypeKind;
        use varn_sem::types::FunctionType;

        let mut out = Vec::new();
        for prop in props {
            let range = prop.range();
            let member = match prop {
                ObjectProp::Property { key, value, .. } => {
                    let value = *value;
                    let name = match key {
                        PropKey::Identifier(s) | PropKey::Str(s) => Arc::from(s.as_str()),
                        PropKey::Int(_) | PropKey::Computed(_) => continue,
                    };
                    let ty = self.infer_expr_type_self(value);
                    let nested_members =
                        if let ExprKind::Object { properties } = &self.ast_arena.expr(value).kind {
                            let properties = properties.clone();
                            self.collect_object_members(&properties)
                        } else {
                            Vec::new()
                        };

                    let kind = if matches!(self.ty_table.get(ty.0), TypeKind::Fn(_)) {
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
                        PropKey::Identifier(s) | PropKey::Str(s) => Arc::from(s.as_str()),
                        PropKey::Int(_) | PropKey::Computed(_) => continue,
                    };
                    let ret_ty = ret_ann
                        .as_ref()
                        .map(|ann| self.resolve_type(ann))
                        .unwrap_or(Type::Dynamic);

                    let fn_params: Vec<_> = params
                        .iter()
                        .map(|p| self.function_param(p, crate::ParamSite::Closure))
                        .collect();

                    let ty = Type::fn_(
                        FunctionType {
                            params: fn_params,
                            return_type: ret_ty.0,
                            is_arrow: false,
                            type_params: Vec::new(),
                        },
                        &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                    );

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
                        PropKey::Identifier(s) | PropKey::Str(s) => Arc::from(s.as_str()),
                        PropKey::Int(_) | PropKey::Computed(_) => continue,
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
                        PropKey::Identifier(s) | PropKey::Str(s) => Arc::from(s.as_str()),
                        PropKey::Int(_) | PropKey::Computed(_) => continue,
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
                _ => continue,
            };
            if let Some(m) = member {
                out.push(m);
            }
        }
        out
    }
}
