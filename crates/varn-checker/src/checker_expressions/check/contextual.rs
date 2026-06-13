use crate::binder::{pattern_lead_name, BindResult};
use crate::checker::Checker;
use crate::types::{FunctionType, ObjectTypeMember, Type, TypeContext};
use varn_core::ast::{ArrayEl, ObjectProp, Param, PropKey};
use varn_core::{Diagnostic, ErrorCode, TypeKind};

impl Checker {
    pub(super) fn apply_contextual_arrow_params(
        &mut self,
        params: &[Param],
        expected_fn: &FunctionType,
        bind: &BindResult,
    ) {
        for (ap, ep) in params.iter().zip(expected_fn.params.iter()) {
            let has_ann = ap.type_ann.is_some()
                || matches!(
                    &ap.pattern,
                    varn_core::ast::Pattern::Identifier {
                        type_ann: Some(_),
                        ..
                    }
                );
            if has_ann || ep.ty.is_dynamic() {
                continue;
            }

            let name = pattern_lead_name(&ap.pattern);
            let scope = bind.scopes.get(self.current_scope);
            if let Some(sym_id) = scope.resolve(name, &bind.scopes) {
                self.symbol_types.insert(sym_id, ep.ty.clone());
                self.mark_infer_env_dirty();
            }
        }
    }

    pub(super) fn check_array_with_context(&mut self, elements: &[ArrayEl], bind: &BindResult) {
        let elem_expected = self.expected_type.as_ref().and_then(|t| match &t.0 {
            TypeKind::Array(inner) => Some(*inner.clone()),
            TypeKind::Generic(name, args, _)
                if name.as_ref() == varn_core::IntrinsicType::Array.as_str() && args.len() == 1 =>
            {
                Some(args[0].clone())
            }
            _ => None,
        });

        for el in elements {
            match el {
                ArrayEl::Expr(e) => {
                    self.with_expected(elem_expected.clone(), |c| c.check_expr(e, bind));
                    if let Some(expected) = &elem_expected {
                        let actual = self.infer_type(e, bind);
                        if !actual.is_dynamic()
                            && !self.types_compatible_cached(expected, &actual, Some(bind))
                        {
                            self.emit(
                                Diagnostic::error(ErrorCode::TypeMismatch, format!(
                                    "type mismatch: array element is '{actual}', expected '{expected}'"
                                ))
                                .with_range(*e.range()),
                            );
                        }
                    }
                }
                ArrayEl::Spread(e) => self.check_expr(e, bind),
                ArrayEl::Hole => {}
            }
        }
    }

    pub(super) fn check_object_with_context(
        &mut self,
        properties: &[ObjectProp],
        bind: &BindResult,
    ) {
        let expected_members: Vec<ObjectTypeMember> = self
            .expected_type
            .as_ref()
            .and_then(|t| {
                let ty = t.non_nullified();
                match &ty.0 {
                    TypeKind::Object(m) => Some(m.clone()),
                    TypeKind::Named(name, origin) | TypeKind::Generic(name, _, origin) => {
                        let members = bind
                            .get_class_members(name, origin.as_deref())
                            .or_else(|| bind.get_interface_members(name, origin.as_deref()))
                            .or_else(|| bind.get_namespace_members(name, origin.as_deref()))
                            .or_else(|| bind.get_enum_members(name, origin.as_deref()))
                            .unwrap_or_default();

                        let mapped = members
                            .into_iter()
                            .map(|m| {
                                if let TypeKind::Fn(ft) = &m.ty.0 {
                                    ObjectTypeMember::Method {
                                        name: m.name,
                                        params: ft.params.clone(),
                                        return_type: ft.return_type.clone(),
                                        optional: m.is_optional,
                                        is_arrow: ft.is_arrow,
                                    }
                                } else {
                                    ObjectTypeMember::Property {
                                        name: m.name,
                                        ty: m.ty,
                                        optional: m.is_optional,
                                        readonly: m.is_readonly,
                                    }
                                }
                            })
                            .collect();
                        Some(mapped)
                    }
                    _ => None,
                }
            })
            .unwrap_or_default();

        for prop in properties {
            match prop {
                ObjectProp::Property { key, value, .. } => {
                    let key_str = prop_key_str(key);
                    let prop_expected = key_str.and_then(|k| {
                        expected_members.iter().find_map(|m| match m {
                            ObjectTypeMember::Property { name, ty, .. } if name.as_ref() == k => {
                                Some(ty.clone())
                            }
                            _ => None,
                        })
                    });
                    self.with_expected(prop_expected.clone(), |c| c.check_expr(value, bind));
                    if let Some(expected) = &prop_expected {
                        let actual = self.infer_type(value, bind);
                        if !actual.is_dynamic()
                            && !self.types_compatible_cached(expected, &actual, Some(bind))
                        {
                            self.emit(
                                Diagnostic::error(
                                    ErrorCode::TypeMismatch,
                                    format!(
                                        "type mismatch: property '{}' is '{}', expected '{}'",
                                        key_str.unwrap_or("?"),
                                        actual,
                                        expected
                                    ),
                                )
                                .with_range(*value.range()),
                            );
                        }
                    }
                }
                ObjectProp::Method {
                    return_type, body, ..
                } => {
                    let saved_expected = self.expected_return_type.take();
                    self.expected_return_type = return_type
                        .as_ref()
                        .map(|rt| self.resolve_type_node_cached(rt, bind));

                    let saved_scope = self.current_scope;
                    if let Some(fn_scope) = self.next_child_scope(bind) {
                        self.current_scope = fn_scope;
                    }
                    self.check_stmt(body, bind);
                    self.current_scope = saved_scope;
                    self.expected_return_type = saved_expected;
                }
                ObjectProp::Getter { body, .. } | ObjectProp::Setter { body, .. } => {
                    self.check_stmt(body, bind)
                }
                ObjectProp::Spread { argument, .. } => self.check_expr(argument, bind),
            }
        }
    }

    pub(super) fn expected_fn_type(&self) -> Option<FunctionType> {
        self.expected_type.as_ref().and_then(|t| {
            if let TypeKind::Fn(ft) = &t.0 {
                Some(ft.clone())
            } else {
                None
            }
        })
    }

    pub(super) fn expected_return_from_fn_type(&self) -> Option<Type> {
        self.expected_fn_type()
            .map(|ft| *ft.return_type)
            .filter(|t| !t.is_dynamic())
    }
}

fn prop_key_str(key: &PropKey) -> Option<&str> {
    match key {
        PropKey::Identifier(s) | PropKey::Str(s) => Some(s.as_str()),
        _ => None,
    }
}
