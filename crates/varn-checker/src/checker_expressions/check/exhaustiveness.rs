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
        if !catch_all {
            let ty = subject_ty.display(&self.ty_table, &bind.interner);
            let message = format!("non-exhaustive match: a match on '{ty}' needs a `_` arm");
            self.report_gap(rec, expr, vec!["_".to_owned()], message, range);
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

        if let Some(members) =
            closed_members(subject_ty, std::sync::Arc::make_mut(&mut self.ty_table))
        {
            let uncovered: Vec<Type> = members
                .into_iter()
                .filter(|m| {
                    !cases.iter().any(|c| {
                        c.guard.is_none()
                            && pattern_covers(&c.pattern, m, &self.ty_table, self.ast_arena, bind)
                    })
                })
                .collect();
            if !uncovered.is_empty() {
                let text = |t: &Type| t.display(&self.ty_table, &bind.interner).to_string();
                let names: Vec<String> = uncovered.iter().map(text).collect();

                let mut missing: Vec<String> = uncovered
                    .iter()
                    .filter(|t| is_literal_pattern(t, &self.ty_table))
                    .map(text)
                    .collect();
                if missing.len() < uncovered.len() {
                    missing.push("_".to_owned());
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

        if let Some(variants) = owner.sum_type_variants.get(type_name.as_ref()) {
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
                            MatchPattern::Sequence(_) | MatchPattern::Type { .. } => false,
                        }
                    })
                })
                .map(|v| v.to_string())
                .collect();
            if !uncovered.is_empty() {
                let missing = uncovered
                    .iter()
                    .map(|v| match owner.sum_variant_fields.get(v.as_str()) {
                        Some(fields) if !fields.is_empty() => {
                            format!("{v}({})", vec!["_"; fields.len()].join(", "))
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
                    let is_variant = owner
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
                            MatchPattern::Identifier(_)
                            | MatchPattern::Record { .. }
                            | MatchPattern::Sequence(_)
                            | MatchPattern::Type { .. } => false,
                        }
                    })
                })
                .map(|v| v.name.to_string())
                .collect();
            if !uncovered.is_empty() {
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
        | TypeKind::Builtin(_)
        | TypeKind::Literal(_)
        | TypeKind::This
        | TypeKind::Array(_)
        | TypeKind::Intersection(_)
        | TypeKind::Tuple(_)
        | TypeKind::Named(..)
        | TypeKind::Generic(..)
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
        MatchPattern::Type { type_name, .. } => {
            let name = bind.interner.resolve(*type_name);
            match table.get(member.0) {
                TypeKind::Named(n, _) | TypeKind::Generic(n, _, _) => {
                    bind.interner.try_resolve(n) == Some(name)
                }
                kind @ TypeKind::Primitive(_)
                | kind @ TypeKind::Builtin(_)
                | kind @ TypeKind::Literal(_)
                | kind @ TypeKind::This
                | kind @ TypeKind::Array(_)
                | kind @ TypeKind::Union(_)
                | kind @ TypeKind::Intersection(_)
                | kind @ TypeKind::Tuple(_)
                | kind @ TypeKind::TemplateLiteral(_)
                | kind @ TypeKind::Fn(_)
                | kind @ TypeKind::Object(_)
                | kind @ TypeKind::Typeof(_)
                | kind @ TypeKind::KeyOf(_)
                | kind @ TypeKind::IndexedAccess { .. }
                | kind @ TypeKind::Mapped { .. }
                | kind @ TypeKind::Conditional { .. }
                | kind @ TypeKind::Infer(_)
                | kind @ TypeKind::EnumVariant { .. }
                | kind @ TypeKind::TypePredicate { .. } => kind.lang_name() == Some(name),
            }
        }
        MatchPattern::Record { .. }
        | MatchPattern::Sequence(_)
        | MatchPattern::EnumVariant { .. } => false,
    }
}
