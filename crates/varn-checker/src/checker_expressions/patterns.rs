use crate::binder::BindResult;
use crate::checker::Checker;
use crate::types::Type;
use varn_core::TypeKind;

type VariantSubst = (
    Vec<(std::sync::Arc<str>, Type)>,
    rustc_hash::FxHashMap<std::sync::Arc<str>, Type>,
);

impl<'r> Checker<'r> {
    pub(crate) fn check_pattern(
        &mut self,
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
                    self.record_type_with_symbol(range.start.offset, *value_ty, id);
                } else {
                    self.record_type(range.start.offset, *value_ty);
                }
            }
            Pattern::Array { elements, rest, .. } => {
                let elem_ty = value_ty.get_array_element_type(&self.ty_table);
                for el in elements.iter().flatten() {
                    self.check_pattern(&el.pattern, &elem_ty, bind);
                }
                if let Some(r) = rest {
                    self.check_pattern(r, value_ty, bind);
                }
            }
            Pattern::Object {
                properties, rest, ..
            } => {
                for prop in properties {
                    let prop_ty = self
                        .find_member_info(value_ty, bind.interner.resolve(prop.key), bind)
                        .map(|(t, _)| t)
                        .unwrap_or(Type::Dynamic);
                    self.check_pattern(&prop.value, &prop_ty, bind);
                }
                if let Some(r) = rest {
                    self.check_pattern(r, &Type::Dynamic, bind);
                }
            }
            Pattern::Rest { argument, .. } => self.check_pattern(argument, value_ty, bind),
            Pattern::Assignment { left, .. } => {
                self.check_pattern(left, value_ty, bind);
            }
        }
    }

    pub(crate) fn check_pattern_match(
        &mut self,
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
                    self.record_type_with_symbol(0, *value_ty, id);
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
                            self.record_type_with_symbol(binding.range.start.offset, resolved, id);
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
                        self.check_pattern_match(sub, &member_ty, bind);
                    } else if key_str != "_" && key_str != "__variant__" {
                        let scope = bind.scopes.get(self.current_scope);
                        if let Some(id) = scope.resolve(*key, &bind.scopes) {
                            self.record_type_with_symbol(0, member_ty, id);
                        }
                    }
                }
            }
            MatchPattern::Sequence(pats) => {
                let elem_ty = value_ty.get_array_element_type(&self.ty_table);
                for p in pats {
                    self.check_pattern_match(p, &elem_ty, bind);
                }
            }
            MatchPattern::Literal(expr) => {
                self.check_expr(*expr, bind);
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
            TypeKind::Primitive(_) | TypeKind::Builtin(_) | TypeKind::Literal(_) | TypeKind::This | TypeKind::Array(_) | TypeKind::Union(_) | TypeKind::Intersection(_) | TypeKind::Tuple(_) | TypeKind::TemplateLiteral(_) | TypeKind::Fn(_) | TypeKind::Object(_) | TypeKind::Typeof(_) | TypeKind::KeyOf(_) | TypeKind::IndexedAccess { .. } | TypeKind::Mapped { .. } | TypeKind::Conditional { .. } | TypeKind::Infer(_) | TypeKind::EnumVariant { .. } | TypeKind::TypePredicate { .. } => (None, Vec::new(), None),
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
