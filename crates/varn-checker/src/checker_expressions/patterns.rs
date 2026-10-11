use crate::checker::recorder::Recorder;
use crate::checker::Checker;
use varn_core::TypeKind;
use varn_sem::bind::BindResult;
use varn_sem::types::Type;

type VariantSubst = (
    Vec<(std::sync::Arc<str>, Type)>,
    rustc_hash::FxHashMap<std::sync::Arc<str>, Type>,
);

impl<'r> Checker<'r> {
    pub(crate) fn check_pattern(
        &mut self,
        rec: &mut Recorder,
        pattern: &varn_core::ast::Pattern,
        value_ty: &Type,
        bind: &BindResult,
    ) {
        use varn_core::ast::Pattern;
        match pattern {
            Pattern::Identifier { name, range, .. } => {
                if bind.interner.resolve(*name) == "_" {
                    return;
                }
                let scope = bind.scopes.get(self.current_scope);
                if let Some(id) = scope.resolve(*name, &bind.scopes) {
                    self.record_type_with_symbol(rec, range.start.offset, *value_ty, id);
                } else {
                    self.record_type(rec, range.start.offset, *value_ty);
                }
            }
            Pattern::Array { elements, rest, .. } => {
                let fixed: Option<Vec<Type>> =
                    if let varn_core::TypeKind::Tuple(items) = self.ty_table.get(value_ty.0) {
                        Some(
                            self.ty_table
                                .get_list(items)
                                .iter()
                                .map(|item| Type::resolved(*item))
                                .collect(),
                        )
                    } else {
                        None
                    };
                let elem_ty = value_ty.get_array_element_type(&self.ty_table);
                for (at, el) in elements.iter().enumerate() {
                    let Some(el) = el else { continue };
                    let item_ty = match &fixed {
                        Some(items) => match items.get(at) {
                            Some(item) => *item,
                            None => {
                                self.emit(
                                    varn_core::Diagnostic::error(
                                        varn_core::ErrorCode::TupleIndexOutOfBounds,
                                        format!(
                                            "this pattern takes element {at}, but '{}' has {} elements",
                                            value_ty.display(&self.ty_table, &bind.interner),
                                            items.len()
                                        ),
                                    )
                                    .with_range(*el.pattern.range()),
                                );
                                Type::Dynamic
                            }
                        },
                        None => elem_ty,
                    };
                    self.check_pattern(rec, &el.pattern, &item_ty, bind);
                }
                if let Some(r) = rest {
                    self.check_pattern(rec, r, value_ty, bind);
                }
            }
            Pattern::Object {
                properties, rest, ..
            } => {
                for prop in properties {
                    let key = bind.interner.resolve(prop.key);
                    let defaulted = matches!(prop.value, Pattern::Assignment { .. });
                    if !defaulted
                        && !value_ty.is_dynamic()
                        && !self.member_exists_cached(value_ty, key, bind)
                    {
                        self.emit(
                            varn_core::Diagnostic::error(
                                varn_core::ErrorCode::MissingProperty,
                                format!(
                                    "property '{key}' does not exist on type '{}'",
                                    value_ty.display(&self.ty_table, &bind.interner)
                                ),
                            )
                            .with_range(*prop.value.range()),
                        );
                    }
                    let prop_ty = self
                        .find_member_info(value_ty, key, bind)
                        .map(|(t, _)| t)
                        .unwrap_or(Type::Dynamic);
                    self.check_pattern(rec, &prop.value, &prop_ty, bind);
                }
                if let Some(r) = rest {
                    self.check_pattern(rec, r, &Type::Dynamic, bind);
                }
            }
            Pattern::Rest { argument, .. } => self.check_pattern(rec, argument, value_ty, bind),
            Pattern::Assignment { left, .. } => {
                self.check_pattern(rec, left, value_ty, bind);
            }
        }
    }

    pub(crate) fn check_pattern_match(
        &mut self,
        rec: &mut Recorder,
        pattern: &varn_core::ast::MatchPattern,
        value_ty: &Type,
        bind: &BindResult,
    ) {
        use varn_core::ast::MatchPattern;
        match pattern {
            MatchPattern::Identifier(name) => {
                if bind.interner.resolve(*name) == "_" {
                    return;
                }
                let scope = bind.scopes.get(self.current_scope);
                if let Some(id) = scope.resolve(*name, &bind.scopes) {
                    self.record_type_with_symbol(rec, 0, *value_ty, id);
                }
            }
            MatchPattern::EnumVariant {
                variant_name,
                bindings,
                ..
            } => {
                let variant_name_str = bind.interner.resolve(*variant_name).to_string();
                let Some((fields, mapping)) =
                    self.variant_fields_with_subst(&variant_name_str, value_ty, bind)
                else {
                    return;
                };
                if let Some(extra) = bindings.get(fields.len()) {
                    self.emit(
                        varn_core::Diagnostic::error(
                            varn_core::ErrorCode::MatchPatternTypeMismatch,
                            format!(
                                "variant '{variant_name_str}' carries {} value(s), but the pattern binds {}",
                                fields.len(),
                                bindings.len()
                            ),
                        )
                        .with_range(extra.range),
                    );
                }
                for (i, binding) in bindings.iter().enumerate() {
                    if bind.interner.resolve(binding.name) == "_" {
                        continue;
                    }
                    if let Some((_, field_ty)) = fields.get(i) {
                        let resolved = if mapping.is_empty() {
                            *field_ty
                        } else {
                            crate::generic_substitution::map_generics_cached(
                                self, field_ty, &mapping,
                            )
                        };
                        let scope = bind.scopes.get(self.current_scope);
                        if let Some(id) = scope.resolve(binding.name, &bind.scopes) {
                            self.record_type_with_symbol(
                                rec,
                                binding.range.start.offset,
                                resolved,
                                id,
                            );
                        }
                    }
                }
            }
            MatchPattern::Record { fields, .. } => {
                for (key, sub_pat) in fields {
                    let key_str = bind.interner.resolve(*key);
                    let member_ty = self
                        .find_member_info(value_ty, key_str, bind)
                        .map(|(t, _)| t)
                        .unwrap_or(Type::Dynamic);
                    if let Some(sub) = sub_pat {
                        self.check_pattern_match(rec, sub, &member_ty, bind);
                    } else if key_str != "_" && key_str != "__variant__" {
                        let scope = bind.scopes.get(self.current_scope);
                        if let Some(id) = scope.resolve(*key, &bind.scopes) {
                            self.record_type_with_symbol(rec, 0, member_ty, id);
                        }
                    }
                }
            }
            MatchPattern::Sequence(pats) => {
                let elem_ty = value_ty.get_array_element_type(&self.ty_table);
                for p in pats {
                    self.check_pattern_match(rec, p, &elem_ty, bind);
                }
            }
            MatchPattern::Literal(expr) => {
                self.check_expr(rec, *expr, bind);
                let given = self.infer_type(rec, *expr, bind);
                if !value_ty.is_dynamic()
                    && !given.is_dynamic()
                    && !self.value_assignable_to(value_ty, &given, Some(*expr), Some(bind))
                {
                    let given_s = given.display(&self.ty_table, &bind.interner);
                    let value_s = value_ty.display(&self.ty_table, &bind.interner);
                    self.emit(
                        varn_core::Diagnostic::error(
                            varn_core::ErrorCode::MatchPatternTypeMismatch,
                            format!(
                                "this pattern is '{given_s}' and can never match a value of type '{value_s}'"
                            ),
                        )
                        .with_range(self.ast_arena.expr(*expr).range),
                    );
                }
            }
            MatchPattern::Wildcard | MatchPattern::Type { .. } => {}
        }
    }

    fn variant_fields_with_subst(
        &mut self,
        variant: &str,
        value_ty: &Type,
        bind: &BindResult,
    ) -> Option<VariantSubst> {
        let (parent, args, origin): (
            Option<std::sync::Arc<str>>,
            Vec<Type>,
            Option<std::sync::Arc<str>>,
        ) = match self.ty_table.get(value_ty.0) {
            TypeKind::Generic(n, a, o) => (
                Some(std::sync::Arc::from(bind.interner.resolve(n))),
                self.ty_table
                    .get_list(a)
                    .iter()
                    .map(|id| Type::resolved(*id))
                    .collect(),
                o.map(|o| std::sync::Arc::from(bind.interner.resolve(o))),
            ),
            TypeKind::Named(n, o) => (
                Some(std::sync::Arc::from(bind.interner.resolve(n))),
                Vec::new(),
                o.map(|o| std::sync::Arc::from(bind.interner.resolve(o))),
            ),
            TypeKind::Primitive(_)
            | TypeKind::Builtin(_)
            | TypeKind::Literal(_)
            | TypeKind::This
            | TypeKind::Array(_)
            | TypeKind::Union(_)
            | TypeKind::Intersection(_)
            | TypeKind::Tuple(_)
            | TypeKind::TemplateLiteral(_)
            | TypeKind::Fn(_)
            | TypeKind::Object(_)
            | TypeKind::Typeof(_)
            | TypeKind::KeyOf(_)
            | TypeKind::IndexedAccess { .. }
            | TypeKind::Mapped { .. }
            | TypeKind::Conditional { .. }
            | TypeKind::Infer(_)
            | TypeKind::EnumVariant { .. }
            | TypeKind::TypePredicate { .. } => (None, Vec::new(), None),
        };

        let mut fields = bind.sum_variant_fields.get(variant).cloned();
        let mut def_bind: Option<std::sync::Arc<BindResult>> = None;
        if fields.is_none() {
            if let Some(o) = &origin {
                let mb = self
                    .resolver
                    .module_bind(o.as_ref())
                    .or_else(|| self.resolver.stdlib_bind(o.as_ref()));
                if let Some(mb) = mb {
                    fields = mb.sum_variant_fields.get(variant).cloned();
                    def_bind = Some(mb);
                }
            }
        }
        let fields = fields?;

        let mut mapping = rustc_hash::FxHashMap::default();
        if !args.is_empty() {
            if let Some(p) = &parent {
                let mut params = self.symbol_type_params_any(p.as_ref(), bind);
                if params.is_empty() {
                    if let Some(db) = &def_bind {
                        params = db
                            .interner
                            .get(p.as_ref())
                            .and_then(|atom| {
                                db.scopes.get(db.global_scope).resolve(atom, &db.scopes)
                            })
                            .map(|sid| {
                                db.arena
                                    .get(sid)
                                    .type_params
                                    .iter()
                                    .map(|a| std::sync::Arc::from(db.interner.resolve(*a)))
                                    .collect()
                            })
                            .unwrap_or_default();
                    }
                }
                for (name, arg) in params.iter().zip(args.iter()) {
                    mapping.insert(name.clone(), *arg);
                }
            }
        }
        Some((fields, mapping))
    }
}
