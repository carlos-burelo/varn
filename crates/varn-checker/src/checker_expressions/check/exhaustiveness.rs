use super::pattern_cover::{lengths_covered, Cover};
use crate::checker::recorder::Recorder;
use crate::checker::Checker;
use varn_core::ast::pattern::MatchPattern;
use varn_core::ast::{AstArena, ExprId, ExprKind, MatchCase};
use varn_core::source::SourceRange;
use varn_core::{Diagnostic, ErrorCode, TypeKind, TypeLiteral};
use varn_sem::bind::BindResult;
use varn_sem::types::{CheckerTyTable, Type};

impl<'r> Checker<'r> {
    fn report_gap(
        &mut self,
        rec: &mut Recorder,
        expr: ExprId,
        missing: Vec<String>,
        message: String,
        range: &SourceRange,
    ) {
        self.emit(Diagnostic::error(ErrorCode::NonExhaustiveMatch, message).with_range(*range));
        rec.match_gaps
            .insert(expr.index(), varn_sem::semantic_info::MatchGap { missing });
    }

    fn require_catch_all(
        &mut self,
        rec: &mut Recorder,
        expr: ExprId,
        subject_ty: &Type,
        cases: &[MatchCase],
        range: &SourceRange,
        bind: &BindResult,
    ) {
        let catch_all = cases.iter().any(|c| {
            c.guard.is_none()
                && matches!(
                    c.pattern,
                    MatchPattern::Wildcard | MatchPattern::Identifier(_)
                )
        });
        if !catch_all && !self.matrix_exhaustive(cases, subject_ty, bind) {
            let ty = subject_ty.display(&self.ty_table, &bind.interner);
            let message = format!("non-exhaustive match: a match on '{ty}' needs a `_` arm");
            self.report_gap(
                rec,
                expr,
                vec![varn_core::TokenKind::Placeholder.as_str().to_owned()],
                message,
                range,
            );
        }
    }

    pub(super) fn check_match_exhaustiveness(
        &mut self,
        rec: &mut Recorder,
        expr: ExprId,
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
        let irrefutable_arms: Vec<&MatchCase> =
            cases.iter().filter(|c| c.guard.is_none()).collect();
        for case in irrefutable_arms {
            if self.irrefutable_for(&case.pattern, subject_ty, bind) {
                return;
            }
        }

        let array_cover: Vec<Cover> = cases
            .iter()
            .filter(|c| c.guard.is_none())
            .map(|c| self.case_cover(&c.pattern, subject_ty, bind))
            .collect();
        let lengths_done = lengths_covered(&array_cover);
        if matches!(self.ty_table.get(subject_ty.0), TypeKind::Array(_)) {
            if !lengths_done {
                self.require_catch_all(rec, expr, subject_ty, cases, range, bind);
            }
            return;
        }

        if let Some(members) =
            closed_members(subject_ty, std::sync::Arc::make_mut(&mut self.ty_table))
        {
            let uncovered: Vec<Type> = members
                .into_iter()
                .filter(|m| {
                    if matches!(self.ty_table.get(m.0), TypeKind::Array(_)) {
                        return !lengths_done;
                    }
                    !cases.iter().any(|c| {
                        c.guard.is_none()
                            && pattern_covers(&c.pattern, m, &self.ty_table, self.ast_arena, bind)
                    })
                })
                .collect();
            if !uncovered.is_empty() && !self.matrix_exhaustive(cases, subject_ty, bind) {
                let text = |t: &Type| t.display(&self.ty_table, &bind.interner).to_string();
                let names: Vec<String> = uncovered.iter().map(text).collect();

                let mut missing: Vec<String> = uncovered
                    .iter()
                    .filter(|t| is_literal_pattern(t, &self.ty_table))
                    .map(text)
                    .collect();
                if missing.len() < uncovered.len() {
                    missing.push(varn_core::TokenKind::Placeholder.as_str().to_owned());
                }
                let message = format!(
                    "non-exhaustive match: missing cases for {}",
                    names.join(", ")
                );
                self.report_gap(rec, expr, missing, message, range);
            }
            return;
        }

        let (TypeKind::Named(type_name_atom, origin_atom)
        | TypeKind::Generic(type_name_atom, _, origin_atom)) = self.ty_table.get(subject_ty.0)
        else {
            self.require_catch_all(rec, expr, subject_ty, cases, range, bind);
            return;
        };
        let type_name: std::sync::Arc<str> = self.resolve_bind_atom(bind, type_name_atom);

        let foreign = origin_atom.and_then(|o| {
            let origin = self.resolve_bind_atom(bind, o);
            self.resolver
                .module_bind(&origin)
                .or_else(|| self.resolver.stdlib_bind(&origin))
        });
        let owner: &BindResult = foreign.as_deref().unwrap_or(bind);

        let covers: Vec<Cover> = cases
            .iter()
            .filter(|c| c.guard.is_none())
            .map(|c| self.case_cover(&c.pattern, subject_ty, bind))
            .collect();
        let covered = |name: &str| {
            covers.iter().any(|cover| match cover {
                Cover::All => true,
                Cover::Variant(covered) => covered == name,
                Cover::Exact(_) | Cover::AtLeast(_) | Cover::Nothing => false,
            })
        };

        if let Some(variants) = owner.sum_type_variants.get(type_name.as_ref()) {
            let uncovered: Vec<String> = variants
                .iter()
                .filter(|vname| !covered(vname.as_ref()))
                .map(|v| v.to_string())
                .collect();
            if !uncovered.is_empty() && !self.matrix_exhaustive(cases, subject_ty, bind) {
                let missing = uncovered
                    .iter()
                    .map(|v| match owner.sum_variant_fields.get(v.as_str()) {
                        Some(fields) if !fields.is_empty() => {
                            format!(
                                "{v}({})",
                                vec![varn_core::TokenKind::Placeholder.as_str(); fields.len()]
                                    .join(", ")
                            )
                        }
                        None | Some(_) => v.clone(),
                    })
                    .collect();
                let message = format!(
                    "non-exhaustive match: missing cases for {}",
                    uncovered.join(", ")
                );
                self.report_gap(rec, expr, missing, message, range);
            }
            return;
        }

        if let Some(variants) = owner.get_enum_members_local(type_name.as_ref()) {
            let uncovered: Vec<String> = variants
                .iter()
                .filter(|v| {
                    owner
                        .sum_variant_parent
                        .get(v.name.as_ref())
                        .is_some_and(|parent| parent.as_ref() == type_name.as_ref())
                })
                .filter(|v| !covered(v.name.as_ref()))
                .map(|v| v.name.to_string())
                .collect();
            if !uncovered.is_empty() && !self.matrix_exhaustive(cases, subject_ty, bind) {
                let message = format!(
                    "non-exhaustive match: missing cases for {}",
                    uncovered.join(", ")
                );
                self.report_gap(rec, expr, uncovered, message, range);
            }
            return;
        }
        self.require_catch_all(rec, expr, subject_ty, cases, range, bind);
    }
}

fn closed_members(subject: &Type, table: &mut CheckerTyTable) -> Option<Vec<Type>> {
    let split_bool = |t: Type, out: &mut Vec<Type>, table: &mut CheckerTyTable| {
        if t == Type::Bool {
            out.push(Type::literal(TypeLiteral::Bool(true), table));
            out.push(Type::literal(TypeLiteral::Bool(false), table));
        } else {
            out.push(t);
        }
    };
    let mut out = Vec::new();
    match table.get(subject.0) {
        TypeKind::Union(list) => {
            for id in table.get_list(list).to_vec() {
                split_bool(Type::resolved(id), &mut out, table);
            }
        }
        TypeKind::Primitive(varn_core::LangPrimitive::Bool) => {
            split_bool(Type::Bool, &mut out, table)
        }
        TypeKind::Primitive(_)
        | TypeKind::Literal(_)
        | TypeKind::This
        | TypeKind::Array(_)
        | TypeKind::Intersection(_)
        | TypeKind::Tuple(_)
        | TypeKind::Named(..)
        | TypeKind::Generic(..)
        | TypeKind::TemplateLiteral(_)
        | TypeKind::Fn(_)
        | TypeKind::Record(_)
        | TypeKind::Interface(_)
        | TypeKind::Typeof(_)
        | TypeKind::KeyOf(_)
        | TypeKind::IndexedAccess { .. }
        | TypeKind::Mapped { .. }
        | TypeKind::Conditional { .. }
        | TypeKind::Infer(_)
        | TypeKind::EnumVariant { .. }
        | TypeKind::TypePredicate { .. } => return None,
    }
    Some(out)
}

fn is_literal_pattern(t: &Type, table: &CheckerTyTable) -> bool {
    matches!(
        table.get(t.0),
        TypeKind::Literal(_) | TypeKind::Primitive(varn_core::LangPrimitive::Null)
    )
}

fn pattern_covers(
    pattern: &MatchPattern,
    member: &Type,
    table: &CheckerTyTable,
    arena: &AstArena,
    bind: &BindResult,
) -> bool {
    match pattern {
        MatchPattern::Wildcard | MatchPattern::Identifier(_) => true,
        MatchPattern::Literal(e) => match (table.get(member.0), &arena.expr(*e).kind) {
            (TypeKind::Primitive(varn_core::LangPrimitive::Null), ExprKind::NullLiteral) => true,
            (TypeKind::Literal(TypeLiteral::Str(a)), ExprKind::StrLiteral { value }) => {
                bind.interner.try_resolve(a) == Some(value.as_str())
            }
            (TypeKind::Literal(TypeLiteral::Bool(b)), ExprKind::BoolLiteral { value }) => {
                b == *value
            }
            (TypeKind::Literal(TypeLiteral::Char(c)), ExprKind::CharLiteral { value }) => {
                c == *value
            }
            (TypeKind::Literal(TypeLiteral::Int(v)), _) => {
                varn_sem::types::numeric_literal::const_int_value(arena, *e) == Some(v)
            }
            _ => false,
        },
        MatchPattern::Tuple(_)
        | MatchPattern::Array { .. }
        | MatchPattern::Record { .. }
        | MatchPattern::EnumVariant { .. } => false,
    }
}
