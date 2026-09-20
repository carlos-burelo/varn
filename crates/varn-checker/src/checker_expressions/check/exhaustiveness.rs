use crate::binder::BindResult;
use crate::checker::Checker;
use crate::types::Type;
use varn_core::ast::pattern::MatchPattern;
use varn_core::ast::MatchCase;
use varn_core::source::SourceRange;
use varn_core::{Diagnostic, ErrorCode, TypeKind};

impl<'r> Checker<'r> {
    pub(super) fn check_match_exhaustiveness(
        &mut self,
        subject_ty: &Type,
        cases: &[MatchCase],
        range: &SourceRange,
        bind: &BindResult,
    ) {
        let has_catch_all = cases
            .iter()
            .any(|c| matches!(c.pattern, MatchPattern::Wildcard) && c.guard.is_none());
        if has_catch_all {
            return;
        }

        if let TypeKind::Union(list) = self.ty_table.get(subject_ty.0) {
            let members: Vec<Type> = self
                .ty_table
                .get_list(list)
                .iter()
                .map(|id| Type(*id, false))
                .collect();
            let uncovered: Vec<String> = members
                .iter()
                .filter(|m| {
                    !cases
                        .iter()
                        .any(|c| c.guard.is_none() && pattern_covers_type(&c.pattern, m))
                })
                .map(|m| m.display(&self.ty_table, &bind.interner).to_string())
                .collect();
            if !uncovered.is_empty() {
                self.emit(
                    Diagnostic::warning(
                        ErrorCode::NonExhaustiveMatch,
                        format!(
                            "non-exhaustive match: missing cases for {}",
                            uncovered.join(", ")
                        ),
                    )
                    .with_range(*range),
                );
            }
            return;
        }

        let TypeKind::Named(type_name_atom, _) = self.ty_table.get(subject_ty.0) else {
            return;
        };
        let type_name: std::sync::Arc<str> =
            std::sync::Arc::from(bind.interner.resolve(type_name_atom));

        if let Some(variants) = bind.sum_type_variants.get(type_name.as_ref()) {
            let uncovered: Vec<String> = variants
                .iter()
                .filter(|vname| {
                    !cases.iter().any(|c| {
                        if c.guard.is_some() {
                            return false;
                        }
                        match &c.pattern {
                            MatchPattern::Wildcard => true,
                            MatchPattern::Identifier(name) => {
                                bind.interner.resolve(*name) == vname.as_ref()
                            }
                            MatchPattern::EnumVariant { variant_name, .. } => {
                                bind.interner.resolve(*variant_name) == vname.as_ref()
                            }
                            MatchPattern::Literal(lit) => {
                                let arena = self.ast_arena;
                                if let varn_core::ast::ExprKind::Member { property, .. } =
                                    &arena.expr(*lit).kind
                                {
                                    if let varn_core::ast::ExprKind::Identifier { name } =
                                        &arena.expr(*property).kind
                                    {
                                        bind.interner.resolve(*name) == vname.as_ref()
                                    } else {
                                        false
                                    }
                                } else if let varn_core::ast::ExprKind::Identifier { name } =
                                    &arena.expr(*lit).kind
                                {
                                    bind.interner.resolve(*name) == vname.as_ref()
                                } else {
                                    false
                                }
                            }
                            MatchPattern::Record { fields, .. } => {
                                fields.first().is_some_and(|(key, sub)| {
                                    bind.interner.resolve(*key) == "__variant__"
                                        && matches!(sub, Some(MatchPattern::Identifier(n)) if bind.interner.resolve(*n) == vname.as_ref())
                                })
                            }
                            _ => false,
                        }
                    })
                })
                .map(|v| v.to_string())
                .collect();
            if !uncovered.is_empty() {
                self.emit(
                    Diagnostic::warning(
                        ErrorCode::NonExhaustiveMatch,
                        format!(
                            "non-exhaustive match: missing cases for {}",
                            uncovered.join(", ")
                        ),
                    )
                    .with_range(*range),
                );
            }
            return;
        }

        if let Some(variants) = bind.get_enum_members_local(type_name.as_ref()) {
            let uncovered: Vec<String> = variants
                .iter()
                .filter(|v| {
                    let is_variant = bind
                        .sum_variant_parent
                        .get(v.name.as_ref())
                        .is_some_and(|parent| parent.as_ref() == type_name.as_ref());
                    if !is_variant {
                        return false;
                    }
                    !cases.iter().any(|c| {
                        if c.guard.is_some() {
                            return false;
                        }
                        match &c.pattern {
                            MatchPattern::Wildcard => true,
                            MatchPattern::EnumVariant { variant_name, .. } => {
                                let variant_name_str = bind.interner.resolve(*variant_name);
                                let last_part = variant_name_str
                                    .rsplit('.')
                                    .next()
                                    .unwrap_or(variant_name_str);
                                last_part == v.name.as_ref()
                            }
                            MatchPattern::Literal(e) => {
                                use varn_core::ast::ExprKind;
                                let arena = self.ast_arena;
                                if let ExprKind::Member { property, .. } = &arena.expr(*e).kind {
                                    if let ExprKind::Identifier { name } =
                                        &arena.expr(*property).kind
                                    {
                                        bind.interner.resolve(*name) == v.name.as_ref()
                                    } else {
                                        false
                                    }
                                } else if let ExprKind::Identifier { name } = &arena.expr(*e).kind {
                                    bind.interner.resolve(*name) == v.name.as_ref()
                                } else {
                                    false
                                }
                            }
                            _ => false,
                        }
                    })
                })
                .map(|v| v.name.to_string())
                .collect();
            if !uncovered.is_empty() {
                self.emit(
                    Diagnostic::warning(
                        ErrorCode::NonExhaustiveMatch,
                        format!(
                            "non-exhaustive match: missing cases for {}",
                            uncovered.join(", ")
                        ),
                    )
                    .with_range(*range),
                );
            }
        }
    }
}

fn pattern_covers_type(pattern: &MatchPattern, _ty: &Type) -> bool {
    matches!(pattern, MatchPattern::Wildcard)
}
