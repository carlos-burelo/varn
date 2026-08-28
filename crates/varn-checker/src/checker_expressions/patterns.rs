use crate::binder::BindResult;
use crate::checker::Checker;
use crate::types::Type;
use varn_core::TypeKind;

type VariantSubst = (
    Vec<(std::rc::Rc<str>, Type)>,
    rustc_hash::FxHashMap<std::rc::Rc<str>, Type>,
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
                if name.as_ref() == "_" {
                    return;
                }
                let scope = bind.scopes.get(self.current_scope);
                if let Some(id) = scope.resolve(name, &bind.scopes) {
                    self.record_type_with_symbol(range.start.offset, value_ty.clone(), id);
                } else {
                    self.record_type(range.start.offset, value_ty.clone());
                }
            }
            Pattern::Array { elements, rest, .. } => {
                let elem_ty = value_ty.get_array_element_type();
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
                        .find_member_info(value_ty, prop.key.as_ref(), bind)
                        .map(|(t, _)| t)
                        .unwrap_or(Type::Dynamic);
                    self.check_pattern(&prop.value, &prop_ty, bind);
                }
                if let Some(r) = rest {
                    self.check_pattern(r, value_ty, bind);
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
                if name.as_ref() == "_" {
                    return;
                }
                let scope = bind.scopes.get(self.current_scope);
                if let Some(id) = scope.resolve(name, &bind.scopes) {
                    self.record_type_with_symbol(0, value_ty.clone(), id);
                }
            }
            MatchPattern::EnumVariant {
                variant_name,
                bindings,
                ..
            } => {
                let Some((fields, mapping)) =
                    self.variant_fields_with_subst(variant_name.as_ref(), value_ty, bind)
                else {
                    return;
                };
                for (i, binding) in bindings.iter().enumerate() {
                    if binding.name.as_ref() == "_" {
                        continue;
                    }
                    if let Some((_, field_ty)) = fields.get(i) {
                        let resolved = if mapping.is_empty() {
                            field_ty.clone()
                        } else {
                            crate::checker_generics::map_generics_cached(self, field_ty, &mapping)
                        };
                        let scope = bind.scopes.get(self.current_scope);
                        if let Some(id) = scope.resolve(&binding.name, &bind.scopes) {
                            self.record_type_with_symbol(binding.range.start.offset, resolved, id);
                        }
                    }
                }
            }
            MatchPattern::Record { fields, .. } => {
                for (key, sub_pat) in fields {
                    let member_ty = self
                        .find_member_info(value_ty, key.as_ref(), bind)
                        .map(|(t, _)| t)
                        .unwrap_or(Type::Dynamic);
                    if let Some(sub) = sub_pat {
                        self.check_pattern_match(sub, &member_ty, bind);
                    } else if key.as_ref() != "_" && key.as_ref() != "__variant__" {
                        let scope = bind.scopes.get(self.current_scope);
                        if let Some(id) = scope.resolve(key, &bind.scopes) {
                            self.record_type_with_symbol(0, member_ty.clone(), id);
                        }
                    }
                }
            }
            MatchPattern::Sequence(pats) => {
                let elem_ty = value_ty.get_array_element_type();
                for p in pats {
                    self.check_pattern_match(p, &elem_ty, bind);
                }
            }
            MatchPattern::Literal(expr) => {
                self.check_expr(expr, bind);
            }
            _ => {}
        }
    }

    /// Resolve a sum-type variant's field types plus a generic substitution
    /// derived from the matched value's concrete type arguments.
    ///
    /// Variant metadata is looked up in this module's bind first, then in the
    /// module that defines the type (via the scrutinee type's origin), so
    /// imported sum types such as `Result`/`Option` resolve. When the scrutinee
    /// is a generic instantiation (`Result<str, int>`), the enum's type
    /// parameters are mapped to the concrete arguments so `Ok(v)` binds
    /// `v: str` instead of the unsubstituted parameter (which surfaces as
    /// `dynamic`).
    fn variant_fields_with_subst(
        &mut self,
        variant: &str,
        value_ty: &Type,
        bind: &BindResult,
    ) -> Option<VariantSubst> {
        let (parent, args, origin) = match &value_ty.0 {
            TypeKind::Generic(n, a, o) => (Some(n.clone()), a.clone(), o.clone()),
            TypeKind::Named(n, o) => (Some(n.clone()), Vec::new(), o.clone()),
            _ => (None, Vec::new(), None),
        };

        // Fields: this module first, then the module that defines the type.
        let mut fields = bind.sum_variant_fields.get(variant).cloned();
        let mut def_bind: Option<std::rc::Rc<BindResult>> = None;
        if fields.is_none() {
            if let Some(o) = &origin {
                let mb = self.resolver.module_bind(o.as_ref())
                    .or_else(|| self.resolver.stdlib_bind(o.as_ref()));
                if let Some(mb) = mb {
                    fields = mb.sum_variant_fields.get(variant).cloned();
                    def_bind = Some(mb);
                }
            }
        }
        let fields = fields?;

        // {type-param -> concrete arg}, e.g. {T: str, E: int} for Result<str, int>.
        let mut mapping = rustc_hash::FxHashMap::default();
        if !args.is_empty() {
            if let Some(p) = &parent {
                let mut params = self.symbol_type_params_any(p.as_ref(), bind);
                if params.is_empty() {
                    if let Some(db) = &def_bind {
                        params = db
                            .scopes
                            .get(db.global_scope)
                            .resolve(p.as_ref(), &db.scopes)
                            .map(|sid| db.arena.get(sid).type_params.clone())
                            .unwrap_or_default();
                    }
                }
                for (name, arg) in params.iter().zip(args.iter()) {
                    mapping.insert(name.clone(), arg.clone());
                }
            }
        }
        Some((fields, mapping))
    }
}
