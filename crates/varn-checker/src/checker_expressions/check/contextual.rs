use crate::binder::{pattern_lead_name, BindResult};
use crate::checker::Checker;
use crate::types::{FunctionType, ObjectTypeMember, Type, TypeContext};
use varn_core::ast::{ArrayEl, ObjectProp, Param, PropKey};
use varn_core::{Diagnostic, ErrorCode, TypeKind};

impl<'r> Checker<'r> {
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
            let ep_ty = Type(ep.ty, false);
            if has_ann || ep_ty.is_dynamic() {
                continue;
            }

            let name = pattern_lead_name(&ap.pattern, &bind.interner);
            let scope = bind.scopes.get(self.current_scope);
            if let Some(sym_id) = bind
                .interner
                .get(name)
                .and_then(|atom| scope.resolve(atom, &bind.scopes))
            {
                self.symbol_types.insert(sym_id, ep_ty);
                self.mark_infer_env_dirty();
            }
        }
    }

    pub(super) fn check_array_with_context(&mut self, elements: &[ArrayEl], bind: &BindResult) {
        let elem_expected = self
            .expected_type
            .and_then(|t| match self.ty_table.get(t.0) {
                TypeKind::Array(inner) => Some(Type(inner, false)),
                TypeKind::Generic(name, args, _)
                    if bind.interner.get(varn_core::BuiltinType::Array.name())
                        == Some(name)
                        && self.ty_table.get_list(args).len() == 1 =>
                {
                    Some(Type(self.ty_table.get_list(args)[0], false))
                }
                _ => None,
            });

        for el in elements {
            match el {
                ArrayEl::Expr(e) => {
                    self.with_expected(elem_expected, |c| c.check_expr(*e, bind));
                    if let Some(expected) = &elem_expected {
                        let actual = self.infer_type(*e, bind);
                        // `value_assignable_to`, not `types_compatible`: a narrow
                        // element type (`Array<i8>`) can only accept an `int`
                        // literal by checking the literal's value, exactly like a
                        // scalar `let x: i8 = 42` does.
                        if !actual.is_dynamic()
                            && !self.value_assignable_to(expected, &actual, Some(*e), Some(bind))
                        {
                            let actual_s = actual.display(&self.ty_table, &bind.interner);
                            let expected_s = expected.display(&self.ty_table, &bind.interner);
                            self.emit(
                                Diagnostic::error(ErrorCode::TypeMismatch, format!(
                                    "type mismatch: array element is '{actual_s}', expected '{expected_s}'"
                                ))
                                .with_range(self.ast_arena.expr(*e).range),
                            );
                        }
                    }
                }
                ArrayEl::Spread(e) => self.check_expr(*e, bind),
                ArrayEl::Hole => {}
            }
        }
    }

    pub(super) fn check_object_with_context(
        &mut self,
        properties: &[ObjectProp],
        bind: &BindResult,
    ) {
        let expected_members: Vec<ObjectTypeMember> = if let Some(t) = self.expected_type {
            let ty = t.non_nullified(&mut *std::sync::Arc::make_mut(&mut self.ty_table));
            if let Some(cached) = self.expected_object_members_cache.get(&ty) {
                cached.clone()
            } else {
                let ty_kind = self.ty_table.get(ty.0);
                let resolved = match ty_kind {
                    TypeKind::Object(mid) => self.ty_table.get_object_members(mid).to_vec(),
                    TypeKind::Generic(name, args, _)
                        if bind.interner.get(varn_core::BuiltinType::Map.name())
                            == Some(name)
                            && self.ty_table.get_list(args).len() == 2 =>
                    {
                        let arg_ids = self.ty_table.get_list(args).to_vec();
                        vec![ObjectTypeMember::Index {
                            param_name: std::sync::Arc::from("key"),
                            key_ty: arg_ids[0],
                            value_ty: arg_ids[1],
                        }]
                    }
                    TypeKind::Named(name_atom, origin_atom)
                    | TypeKind::Generic(name_atom, _, origin_atom) => {
                        // `bind.interner` is a snapshot from when *this*
                        // module started checking — `name_atom`/`origin_atom`
                        // can come from a `Type` a cross-module lookup built
                        // from a sibling module's (possibly later, bigger)
                        // table. `try_resolve` degrades to a fresh live
                        // snapshot instead of indexing out of bounds; the
                        // live table is guaranteed to have anything ever
                        // actually minted this compilation.
                        let resolve_atom = |a: varn_core::Atom| -> String {
                            bind.interner
                                .try_resolve(a)
                                .map(str::to_string)
                                .unwrap_or_else(|| {
                                    self.resolver.interner_snapshot().resolve(a).to_string()
                                })
                        };
                        let name = resolve_atom(name_atom);
                        let origin: Option<String> = origin_atom.map(resolve_atom);
                        let view = crate::binder::BindView::new(bind, self.resolver);
                        let members = view
                            .get_class_members(&name, origin.as_deref())
                            .or_else(|| view.get_interface_members(&name, origin.as_deref()))
                            .or_else(|| view.get_namespace_members(&name, origin.as_deref()))
                            .or_else(|| view.get_enum_members(&name, origin.as_deref()))
                            .unwrap_or_default();

                        members
                            .into_iter()
                            .map(|m| {
                                let m_kind = self.ty_table.get(m.ty.0);
                                if let TypeKind::Fn(fid) = m_kind {
                                    let ft = self.ty_table.get_function(fid).clone();
                                    ObjectTypeMember::Method {
                                        name: m.name,
                                        params: ft.params.clone(),
                                        return_type: ft.return_type,
                                        optional: m.is_optional,
                                        is_arrow: ft.is_arrow,
                                    }
                                } else {
                                    ObjectTypeMember::Property {
                                        name: m.name,
                                        ty: m.ty.0,
                                        optional: m.is_optional,
                                        readonly: m.is_readonly,
                                    }
                                }
                            })
                            .collect()
                    }
                    _ => Vec::new(),
                };
                self.expected_object_members_cache
                    .insert(ty, resolved.clone());
                resolved
            }
        } else {
            Vec::new()
        };

        for prop in properties {
            match prop {
                ObjectProp::Property { key, value, .. } => {
                    let key_str = prop_key_str(key);
                    let prop_expected = key_str.and_then(|k| {
                        expected_members.iter().find_map(|m| match m {
                            ObjectTypeMember::Property { name, ty, .. } if name.as_ref() == k => {
                                Some(Type(*ty, false))
                            }
                            ObjectTypeMember::Index { value_ty, .. } => {
                                Some(Type(*value_ty, false))
                            }
                            _ => None,
                        })
                    });
                    self.with_expected(prop_expected, |c| c.check_expr(*value, bind));
                    if let Some(expected) = &prop_expected {
                        let actual = self.infer_type(*value, bind);
                        if !actual.is_dynamic()
                            && !self.value_assignable_to(
                                expected,
                                &actual,
                                Some(*value),
                                Some(bind),
                            )
                        {
                            let actual_s = actual.display(&self.ty_table, &bind.interner);
                            let expected_s = expected.display(&self.ty_table, &bind.interner);
                            self.emit(
                                Diagnostic::error(
                                    ErrorCode::TypeMismatch,
                                    format!(
                                        "type mismatch: property '{}' is '{}', expected '{}'",
                                        key_str.unwrap_or("?"),
                                        actual_s,
                                        expected_s
                                    ),
                                )
                                .with_range(self.ast_arena.expr(*value).range),
                            );
                        }
                    }
                }
                ObjectProp::Method {
                    return_type,
                    body,
                    is_async,
                    ..
                } => {
                    let saved_expected = self.expected_return_type.take();
                    self.expected_return_type = return_type.as_ref().map(|rt| {
                        let ty = self.resolve_type_node_cached(rt, bind);
                        if *is_async {
                            crate::types::awaited(&ty, &self.ty_table, &bind.interner)
                        } else {
                            ty
                        }
                    });

                    let saved_scope = self.current_scope;
                    if let Some(fn_scope) = self.next_child_scope(bind) {
                        self.current_scope = fn_scope;
                    }
                    self.in_function_body(|c| c.check_stmt(*body, bind));
                    self.current_scope = saved_scope;
                    self.expected_return_type = saved_expected;
                }
                // Accessors parse inside an object literal, but the compiler has
                // no `HirObjectProp` for them: they are lowered away, and the
                // property reads back as `null`. Silently dropping a written
                // accessor is worse than not having them, so say so.
                ObjectProp::Getter { body, range, .. } | ObjectProp::Setter { body, range, .. } => {
                    self.emit(
                        Diagnostic::error(
                            ErrorCode::UnsupportedExpression,
                            "getters and setters are not supported in object literals — \
                             declare a class, or use a method"
                                .to_string(),
                        )
                        .with_range(*range),
                    );
                    let saved_expected = self.expected_return_type.take();
                    self.in_function_body(|c| c.check_stmt(*body, bind));
                    self.expected_return_type = saved_expected;
                }
                ObjectProp::Spread { argument, .. } => self.check_expr(*argument, bind),
            }
        }
    }

    pub(super) fn expected_fn_type(&self) -> Option<FunctionType> {
        self.expected_type.and_then(|t| {
            if let TypeKind::Fn(fid) = self.ty_table.get(t.0) {
                Some(self.ty_table.get_function(fid).clone())
            } else {
                None
            }
        })
    }

    pub(super) fn expected_return_from_fn_type(&self) -> Option<Type> {
        self.expected_fn_type()
            .map(|ft| Type(ft.return_type, false))
            .filter(|t| !t.is_dynamic())
    }
}

fn prop_key_str(key: &PropKey) -> Option<&str> {
    match key {
        PropKey::Identifier(s) | PropKey::Str(s) => Some(s.as_str()),
        _ => None,
    }
}
