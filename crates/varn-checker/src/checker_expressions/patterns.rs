use crate::binder::BindResult;
use crate::checker::Checker;
use crate::types::Type;

impl Checker {
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
                if let Some(fields) = bind.sum_variant_fields.get(variant_name.as_ref()) {
                    for (i, binding) in bindings.iter().enumerate() {
                        if binding.name.as_ref() == "_" {
                            continue;
                        }
                        if let Some((_, field_ty)) = fields.get(i) {
                            let scope = bind.scopes.get(self.current_scope);
                            if let Some(id) = scope.resolve(&binding.name, &bind.scopes) {
                                self.record_type_with_symbol(
                                    binding.range.start.offset,
                                    field_ty.clone(),
                                    id,
                                );
                            }
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
}
