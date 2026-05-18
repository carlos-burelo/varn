use crate::binder::BindResult;
use crate::checker::Checker;
use crate::types::Type;
use varn_core::ast::pattern::MatchPattern;
use varn_core::ast::{ExprKind, MatchCase};
use varn_core::source::SourceRange;
use varn_core::{Diagnostic, ErrorCode, TypeKind};

impl Checker {
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

        if let TypeKind::Union(members) = &subject_ty.0 {
            let uncovered: Vec<String> = members
                .iter()
                .filter(|m| {
                    !cases
                        .iter()
                        .any(|c| c.guard.is_none() && pattern_covers_type(&c.pattern, m))
                })
                .map(|m| m.to_string())
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

        let TypeKind::Named(type_name, _) = &subject_ty.0 else {
            return;
        };

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
                            MatchPattern::Identifier(name) => name.as_ref() == vname.as_ref(),
                            MatchPattern::Record { fields, .. } => {
                                fields.first().is_some_and(|(key, sub)| {
                                    key.as_ref() == "__variant__"
                                        && matches!(sub, Some(MatchPattern::Identifier(n)) if n.as_ref() == vname.as_ref())
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
                    !cases.iter().any(|c| {
                        if c.guard.is_some() {
                            return false;
                        }
                        match &c.pattern {
                            MatchPattern::Wildcard => true,
                            MatchPattern::EnumVariant { variant_name, .. } => {
                                variant_name.as_ref() == v.name.as_ref()
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
            return;
        }
    }
}

fn pattern_covers_type(pattern: &MatchPattern, ty: &Type) -> bool {
    match pattern {
        MatchPattern::Wildcard => true,
        MatchPattern::Literal(e) => match &e.kind {
            ExprKind::StrLiteral { value } => {
                matches!(&ty.0, TypeKind::LiteralStr(s) if s.as_ref() == value)
            }
            ExprKind::IntLiteral { value, .. } => {
                matches!(&ty.0, TypeKind::LiteralInt(v) if v == value)
            }
            ExprKind::BoolLiteral { value } => {
                matches!(&ty.0, TypeKind::LiteralBool(v) if v == value)
            }
            _ => false,
        },
        _ => false,
    }
}
