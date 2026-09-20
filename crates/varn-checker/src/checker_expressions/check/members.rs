use crate::binder::BindResult;
use crate::checker::Checker;
use crate::checker_expressions::helpers::closest_in_list;
use crate::types::{ObjectTypeMember, Type};
use varn_core::ast::operators::Visibility;
use varn_core::ast::{ExprId, ExprKind};
use varn_core::source::SourceRange;
use varn_core::{Diagnostic, ErrorCode, Suggestion, TypeKind};

impl<'r> Checker<'r> {
    pub(super) fn check_extension_assignment(&mut self, target: ExprId, bind: &BindResult) {
        let arena = self.ast_arena;
        let target_range = arena.expr(target).range;
        let ExprKind::Member {
            object,
            property,
            computed: false,
            ..
        } = &arena.expr(target).kind
        else {
            return;
        };
        let (object, property) = (*object, *property);
        let ExprKind::Identifier { name: prop_name } = &arena.expr(property).kind else {
            return;
        };
        let prop_name = bind.interner.resolve(*prop_name);

        let obj_ty = self.infer_type(object, bind);
        let non_null = obj_ty.non_nullified(&mut *std::sync::Arc::make_mut(&mut self.ty_table));
        if let Some(tn) = extension_type_name(self, &non_null, &self.ty_table, bind) {
            if let Some(setter_map) = bind.extensions.setters.get(tn.as_ref()) {
                if let Some(mangled) = setter_map.get(prop_name) {
                    self.extension_set_members
                        .insert(target_range.start.offset, mangled.clone());
                }
            }
        }
        if let Some(ObjectTypeMember::Property { readonly: true, .. }) =
            self.find_member(&obj_ty, prop_name, bind)
        {
            self.emit(
                Diagnostic::error(
                    ErrorCode::NotAssignable,
                    format!("cannot assign to readonly property '{prop_name}'"),
                )
                .with_range(target_range),
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn check_member_expr(
        &mut self,
        expr: ExprId,
        object: ExprId,
        property: ExprId,
        computed: bool,
        optional: bool,
        range: &SourceRange,
        bind: &BindResult,
    ) {
        let arena = self.ast_arena;
        let property_range = arena.expr(property).range;
        self.check_expr(object, bind);
        if computed {
            if matches!(arena.expr(property).kind, ExprKind::Range { .. }) {
                self.check_expr(property, bind);
                return;
            }
            let obj_ty = self.infer_type(object, bind);
            let check_ty = obj_ty.non_nullified(&mut *std::sync::Arc::make_mut(&mut self.ty_table));
            let check_kind = self.ty_table.get(check_ty.0);
            let key_expected = match check_kind {
                TypeKind::Generic(name, args, _)
                    if bind.interner.get(varn_core::IntrinsicType::Map.as_str()) == Some(name) =>
                {
                    let arg_ids = self.ty_table.get_list(args).to_vec();
                    if arg_ids.len() == 2 {
                        Some(Type(arg_ids[0], false))
                    } else if arg_ids.len() == 1 {
                        Some(Type::Str)
                    } else {
                        None
                    }
                }
                TypeKind::Object(mid) => {
                    self.ty_table
                        .get_object_members(mid)
                        .iter()
                        .find_map(|m| match m {
                            ObjectTypeMember::Index { key_ty, .. } => Some(Type(*key_ty, false)),
                            _ => None,
                        })
                }
                TypeKind::Array(_) | TypeKind::Intrinsic(varn_core::TypeTag::Bytes) => {
                    Some(Type::Int)
                }
                _ => None,
            };
            if let Some(expected_k) = key_expected {
                self.with_expected(Some(expected_k), |c| c.check_expr(property, bind));
                let actual_k = self.infer_type(property, bind);
                let is_range_slice = matches!(
                    check_kind,
                    TypeKind::Array(_)
                        | TypeKind::Intrinsic(varn_core::TypeTag::Str | varn_core::TypeTag::Bytes)
                ) && matches!(
                    self.ty_table.get(actual_k.0),
                    TypeKind::Intrinsic(varn_core::TypeTag::Range)
                );
                if !actual_k.is_dynamic()
                    && !is_range_slice
                    && !self.types_compatible_cached(&expected_k, &actual_k, Some(bind))
                {
                    self.emit(
                        Diagnostic::error(
                            ErrorCode::TypeMismatch,
                            format!(
                                "type mismatch: index key is '{}', expected '{}'",
                                actual_k.display(&self.ty_table, &bind.interner),
                                expected_k.display(&self.ty_table, &bind.interner)
                            ),
                        )
                        .with_range(property_range),
                    );
                }
            } else {
                self.check_expr(property, bind);
            }
            return;
        }

        let ExprKind::Identifier { name: prop_name } = &arena.expr(property).kind else {
            let prop_ty = self.infer_type(expr, bind);
            self.record_type(property_range.start.offset, prop_ty);
            return;
        };
        let prop_name = bind.interner.resolve(*prop_name);

        let obj_ty = self.infer_type(object, bind);
        if !optional && obj_ty.is_nullable(&self.ty_table) {
            self.emit(
                Diagnostic::error(
                    ErrorCode::PossibleNullDereference,
                    format!(
                        "object is possibly null: cannot access property '{}' on nullable type '{}'",
                        prop_name,
                        obj_ty.display(&self.ty_table, &bind.interner)
                    ),
                )
                .with_suggestion(Suggestion::new(
                    "use optional chaining '?.' to safely access properties on a nullable object",
                ))
                .with_range(*range),
            );
        }

        let check_ty = obj_ty.non_nullified(&mut *std::sync::Arc::make_mut(&mut self.ty_table));

        if let Some((ty, maybe_sid)) = self.find_member_info(&check_ty, prop_name, bind) {
            if let Some(sid) = maybe_sid {
                self.record_member_type(property_range.start.offset, ty, sid);
            } else {
                self.record_type(property_range.start.offset, ty);
            }
        } else {
            let prop_ty = self.infer_type(expr, bind);
            self.record_type(property_range.start.offset, prop_ty);
        }
        let should_check = !matches!(
            self.ty_table.get(check_ty.0),
            TypeKind::Intrinsic(varn_core::TypeTag::Never)
        );

        if let Some(tn) = extension_type_name(self, &check_ty, &self.ty_table, bind) {
            if let Some(getter_map) = bind.extensions.getters.get(tn.as_ref()) {
                if let Some(mangled) = getter_map.get(prop_name) {
                    self.extension_members
                        .insert(property_range.start.offset, mangled.clone());
                }
            } else if let Some(method_map) = bind.extensions.methods.get(tn.as_ref()) {
                if let Some(mangled) = method_map.get(prop_name) {
                    self.extension_members
                        .insert(property_range.start.offset, mangled.clone());
                }
            }
        }

        if should_check && !self.member_exists_cached(&check_ty, prop_name, bind) {
            let candidates = self.collect_member_names(&check_ty, bind);
            let suggestion = closest_in_list(prop_name, &candidates)
                .map(|c| Suggestion::did_you_mean(c, *range));
            let mut diag = Diagnostic::error(
                ErrorCode::MissingProperty,
                format!(
                    "property '{prop_name}' does not exist on type '{}'",
                    check_ty.display(&self.ty_table, &bind.interner)
                ),
            )
            .with_range(*range);
            if let Some(s) = suggestion {
                diag = diag.with_suggestion(s);
            }
            self.emit(diag);
        }

        if self.record_expr_types {
            let final_mem_ty = self
                .find_member_info(&check_ty, prop_name, bind)
                .map(|(t, _)| t)
                .unwrap_or_else(|| self.infer_type(expr, bind));

            let is_static = if let ExprKind::Identifier { name } = &arena.expr(object).kind {
                bind.scopes
                    .get(bind.global_scope)
                    .resolve(*name, &bind.scopes)
                    .map(|sid| {
                        matches!(
                            bind.arena.get(sid).kind,
                            crate::symbol::SymbolKind::Class
                                | crate::symbol::SymbolKind::Interface
                                | crate::symbol::SymbolKind::Enum
                                | crate::symbol::SymbolKind::Namespace
                                | crate::symbol::SymbolKind::Struct
                        )
                    })
                    .unwrap_or(false)
            } else {
                false
            };

            let check_kind = self.ty_table.get(check_ty.0);
            let is_enum = matches!(check_kind, TypeKind::EnumVariant { .. })
                || if let TypeKind::Named(n, _) = check_kind {
                    let n_str = self.resolve_bind_atom(bind, n);
                    bind.interner
                        .get(&n_str)
                        .and_then(|atom| {
                            bind.scopes
                                .get(bind.global_scope)
                                .resolve(atom, &bind.scopes)
                        })
                        .map(|sid| bind.arena.get(sid).kind == crate::symbol::SymbolKind::Enum)
                        .unwrap_or(false)
                } else {
                    false
                };

            let final_mem_kind = self.ty_table.get(final_mem_ty.0);
            let member_kind = if is_enum {
                crate::semantic_info::ResolvedMemberKind::EnumMember
            } else if self
                .extension_members
                .contains_key(&property_range.start.offset)
            {
                if matches!(final_mem_kind, TypeKind::Fn(_)) {
                    crate::semantic_info::ResolvedMemberKind::ExtensionMethod
                } else {
                    crate::semantic_info::ResolvedMemberKind::ExtensionProperty
                }
            } else if is_static {
                if matches!(final_mem_kind, TypeKind::Fn(_)) {
                    crate::semantic_info::ResolvedMemberKind::StaticMethod
                } else {
                    crate::semantic_info::ResolvedMemberKind::StaticProperty
                }
            } else if matches!(final_mem_kind, TypeKind::Fn(_)) {
                crate::semantic_info::ResolvedMemberKind::Method
            } else {
                crate::semantic_info::ResolvedMemberKind::Property
            };

            let origin_module = match check_kind {
                TypeKind::Named(_, orig) | TypeKind::Generic(_, _, orig) => {
                    orig.map(|o| self.resolve_bind_atom(bind, o))
                }
                TypeKind::Intrinsic(tag) => Some(std::sync::Arc::from(match tag {
                    varn_core::TypeTag::Map => "core:map",
                    varn_core::TypeTag::Set => "core:set",
                    varn_core::TypeTag::Range => "core:range",
                    varn_core::TypeTag::Array => "core:array",
                    varn_core::TypeTag::Str => "core:str",
                    varn_core::TypeTag::Bytes => "core:bytes",
                    varn_core::TypeTag::TaskHandle => "core:task",
                    _ => "core:primitives",
                })),
                _ => None,
            };

            self.member_resolutions.insert(
                property_range.start.offset,
                crate::semantic_info::MemberResolution {
                    receiver_ty: check_ty,
                    member_name: std::sync::Arc::from(prop_name),
                    member_kind,
                    member_ty: final_mem_ty,
                    origin_module,
                    def_range: None,
                    doc: None,
                },
            );
        }

        let obj_kind = self.ty_table.get(obj_ty.0);
        let class_name = match obj_kind {
            TypeKind::Named(n, _origin) | TypeKind::Generic(n, _, _origin) => {
                Some(self.resolve_bind_atom(bind, n).to_string())
            }
            _ => None,
        };
        if let Some(class_name) = class_name {
            self.check_member_visibility(&class_name, prop_name, range, bind);
        }
    }

    fn check_member_visibility(
        &mut self,
        class_name: &str,
        prop_name: &str,
        range: &SourceRange,
        bind: &BindResult,
    ) {
        let Some(members) = bind.get_class_entry(class_name) else {
            return;
        };
        let Some(m) = members
            .members
            .iter()
            .find(|m| m.name.as_ref() == prop_name)
        else {
            return;
        };

        match m.visibility {
            Some(Visibility::Private) => {
                if self.current_class.as_deref() != Some(class_name) {
                    self.emit(
                        Diagnostic::error(ErrorCode::PrivateMemberAccess, format!(
                            "property '{prop_name}' is private and only accessible within class '{class_name}'"
                        ))
                        .with_range(*range),
                    );
                }
            }
            Some(Visibility::Protected) => {
                let current_class = self.current_class.as_deref();
                let is_authorized = current_class.is_some_and(|c| {
                    c == class_name || self.is_subclass_or_same(c, class_name, bind)
                });
                if !is_authorized {
                    self.emit(
                        Diagnostic::error(ErrorCode::ProtectedMemberAccess, format!(
                            "property '{prop_name}' is protected and only accessible within class '{class_name}' and its subclasses"
                        ))
                        .with_range(*range),
                    );
                }
            }
            _ => {}
        }
    }
}

pub(crate) fn extension_type_name(
    checker: &Checker,
    ty: &Type,
    table: &crate::types::CheckerTyTable,
    bind: &BindResult,
) -> Option<std::sync::Arc<str>> {
    match table.get(ty.0) {
        TypeKind::Named(n, _) | TypeKind::Generic(n, _, _) => {
            Some(checker.resolve_bind_atom(bind, n))
        }
        TypeKind::Intrinsic(tag) => Some(std::sync::Arc::from(tag.name())),
        _ => None,
    }
}
