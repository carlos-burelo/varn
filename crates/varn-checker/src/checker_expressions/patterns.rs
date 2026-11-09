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
                if bind.interner.resolve(*name) == varn_core::TokenKind::Placeholder.as_str() {
                    return;
                }
                let scope = bind.scopes.get(self.current_scope);
                if let Some(id) = scope.resolve(*name, &bind.scopes) {
                    self.record_type_with_symbol(rec, range.start.offset, *value_ty, id);
                } else {
                    self.record_type(rec, range.start.offset, *value_ty);
                }
            }
            Pattern::Array { elements, rest, .. } | Pattern::Tuple { elements, rest, .. } => {
                let shape = self.sequence_shape(
                    value_ty,
                    matches!(pattern, Pattern::Tuple { .. }),
                    elements.len(),
                    rest.is_some(),
                    *pattern.range(),
                    bind,
                );
                let (fixed, elem_ty) = (shape.fixed, shape.element);
                if !shape.sequence {
                    return;
                }
                for (at, el) in elements.iter().enumerate() {
                    let Some(el) = el else { continue };
                    let item_ty = match &fixed {
                        Some(items) => items.get(at).copied().unwrap_or(Type::Error),
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
                        && !value_ty.is_unresolved()
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
                    let default_ty = match &prop.value {
                        Pattern::Assignment { right, .. } => {
                            Some(self.infer_type(rec, *right, bind))
                        }
                        Pattern::Identifier { .. }
                        | Pattern::Array { .. }
                        | Pattern::Tuple { .. }
                        | Pattern::Object { .. }
                        | Pattern::Rest { .. } => None,
                    };
                    let prop_ty = self
                        .find_member_info(value_ty, key, bind)
                        .map(|(t, _)| t)
                        .or(default_ty)
                        .unwrap_or(Type::Error);
                    self.check_pattern(rec, &prop.value, &prop_ty, bind);
                }
                if let Some(r) = rest {
                    let extracted: Vec<&str> = properties
                        .iter()
                        .map(|prop| bind.interner.resolve(prop.key))
                        .collect();
                    let rest_ty = self.object_rest_type(value_ty, &extracted, *r.range(), bind);
                    self.check_pattern(rec, r, &rest_ty, bind);
                }
            }
            Pattern::Rest { argument, .. } => self.check_pattern(rec, argument, value_ty, bind),
            Pattern::Assignment { left, .. } => {
                self.check_pattern(rec, left, value_ty, bind);
            }
        }
    }

    fn object_rest_type(
        &mut self,
        value_ty: &Type,
        extracted: &[&str],
        range: varn_core::SourceRange,
        bind: &BindResult,
    ) -> Type {
        if value_ty.is_unresolved() {
            return *value_ty;
        }
        if let varn_core::TypeKind::Record(id) | varn_core::TypeKind::Interface(id) =
            self.ty_table.get(value_ty.0)
        {
            let remaining: Vec<varn_sem::types::ObjectTypeMember> = self
                .ty_table
                .get_object_members(id)
                .iter()
                .filter(|m| match m {
                    varn_sem::types::ObjectTypeMember::Property { name, .. }
                    | varn_sem::types::ObjectTypeMember::Method { name, .. } => {
                        !extracted.contains(&name.as_ref())
                    }
                    varn_sem::types::ObjectTypeMember::Callable { .. } => true,
                })
                .cloned()
                .collect();
            return Type::record(remaining, std::sync::Arc::make_mut(&mut self.ty_table));
        }
        self.emit(
            varn_core::Diagnostic::error(
                varn_core::ErrorCode::TypeMismatch,
                format!(
                    "object rest needs an object type, found '{}'",
                    value_ty.display(&self.ty_table, &bind.interner)
                ),
            )
            .with_range(range),
        );
        Type::Error
    }

    pub(crate) fn check_pattern_match(
        &mut self,
        rec: &mut Recorder,
        pattern: &varn_core::ast::MatchPattern,
        value_ty: &Type,
        range: varn_core::SourceRange,
        bind: &BindResult,
    ) {
        use varn_core::ast::{MatchPattern, SequenceRest, VariantPayload};
        match pattern {
            MatchPattern::Wildcard => {}
            MatchPattern::Identifier(name) => {
                if bind.interner.resolve(*name) == varn_core::TokenKind::Placeholder.as_str() {
                    return;
                }
                let scope = bind.scopes.get(self.current_scope);
                if let Some(id) = scope.resolve(*name, &bind.scopes) {
                    self.record_type_with_symbol(rec, 0, *value_ty, id);
                }
            }
            MatchPattern::Tuple(items) => {
                let present = self.present_subject(value_ty);
                let shape = self.sequence_shape(&present, true, items.len(), false, range, bind);
                for (at, item) in items.iter().enumerate() {
                    let item_ty = match &shape.fixed {
                        Some(fixed) => fixed.get(at).copied().unwrap_or(Type::Error),
                        None => shape.element,
                    };
                    self.check_pattern_match(rec, item, &item_ty, range, bind);
                }
            }
            MatchPattern::Array { elements, rest } => {
                let present = self.present_subject(value_ty);
                let shape = self.sequence_shape(
                    &present,
                    false,
                    elements.len(),
                    !matches!(rest, SequenceRest::Closed),
                    range,
                    bind,
                );
                for (at, item) in elements.iter().enumerate() {
                    let item_ty = match &shape.fixed {
                        Some(fixed) => fixed.get(at).copied().unwrap_or(Type::Error),
                        None => shape.element,
                    };
                    self.check_pattern_match(rec, item, &item_ty, range, bind);
                }
                if let SequenceRest::Bind(name) = rest {
                    let rest_ty =
                        Type::array(shape.element, std::sync::Arc::make_mut(&mut self.ty_table));
                    let scope = bind.scopes.get(self.current_scope);
                    if let Some(id) = scope.resolve(*name, &bind.scopes) {
                        self.record_type_with_symbol(rec, 0, rest_ty, id);
                    }
                }
            }
            MatchPattern::Record { fields, .. } => {
                let present = self.present_subject(value_ty);
                for field in fields {
                    let name = bind.interner.resolve(field.name);
                    let member_ty = if present.is_unresolved() {
                        Type::Error
                    } else if let Some((found, _)) = self.find_member_info(&present, name, bind) {
                        found
                    } else {
                        self.emit(
                            varn_core::Diagnostic::error(
                                varn_core::ErrorCode::MissingProperty,
                                format!(
                                    "property '{name}' does not exist on type '{}'",
                                    present.display(&self.ty_table, &bind.interner)
                                ),
                            )
                            .with_range(field.range),
                        );
                        Type::Error
                    };
                    self.check_match_field(rec, field, &member_ty, range, bind);
                }
            }
            MatchPattern::EnumVariant {
                variant_name,
                payload,
                ..
            } => {
                let variant_name_str = bind.interner.resolve(*variant_name).to_string();
                let Some((fields, mapping)) =
                    self.variant_fields_with_subst(&variant_name_str, value_ty, bind)
                else {
                    return;
                };
                let field_ty = |checker: &mut Self, ty: &Type| {
                    if mapping.is_empty() {
                        *ty
                    } else {
                        crate::generic_substitution::map_generics_cached(checker, ty, &mapping)
                    }
                };
                match payload {
                    VariantPayload::Positional(items) => {
                        if items.len() != fields.len() {
                            self.emit(
                                varn_core::Diagnostic::error(
                                    varn_core::ErrorCode::MatchPatternTypeMismatch,
                                    format!(
                                        "variant '{variant_name_str}' carries {} value(s), but the pattern has {}",
                                        fields.len(),
                                        items.len()
                                    ),
                                )
                                .with_range(range),
                            );
                        }
                        for (at, item) in items.iter().enumerate() {
                            let item_ty = match fields.get(at) {
                                Some((_, ty)) => field_ty(self, ty),
                                None => Type::Error,
                            };
                            self.check_pattern_match(rec, item, &item_ty, range, bind);
                        }
                    }
                    VariantPayload::Named { fields: named, .. } => {
                        for field in named {
                            let name = bind.interner.resolve(field.name);
                            let item_ty = match fields.iter().find(|(n, _)| n.as_ref() == name) {
                                Some((_, ty)) => field_ty(self, ty),
                                None => {
                                    self.emit(
                                        varn_core::Diagnostic::error(
                                            varn_core::ErrorCode::MatchPatternTypeMismatch,
                                            format!(
                                                "variant '{variant_name_str}' has no field named '{name}'"
                                            ),
                                        )
                                        .with_range(field.range),
                                    );
                                    Type::Error
                                }
                            };
                            self.check_match_field(rec, field, &item_ty, range, bind);
                        }
                    }
                }
            }
            MatchPattern::Literal(expr) => {
                self.check_expr(rec, *expr, bind);
                let given = self.infer_type(rec, *expr, bind);
                if !value_ty.is_unresolved()
                    && !given.is_unresolved()
                    && !self.value_fits(rec, value_ty, &given, Some(*expr), Some(bind))
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
        }
    }

    fn present_subject(&mut self, value_ty: &Type) -> Type {
        if value_ty.is_unresolved() {
            *value_ty
        } else {
            value_ty.non_nullified(std::sync::Arc::make_mut(&mut self.ty_table))
        }
    }

    fn check_match_field(
        &mut self,
        rec: &mut Recorder,
        field: &varn_core::ast::MatchField,
        field_ty: &Type,
        range: varn_core::SourceRange,
        bind: &BindResult,
    ) {
        match &field.pattern {
            Some(sub) => self.check_pattern_match(rec, sub, field_ty, range, bind),
            None => {
                if bind.interner.resolve(field.name) == varn_core::TokenKind::Placeholder.as_str() {
                    return;
                }
                let scope = bind.scopes.get(self.current_scope);
                if let Some(id) = scope.resolve(field.name, &bind.scopes) {
                    self.record_type_with_symbol(rec, field.range.start.offset, *field_ty, id);
                }
            }
        }
    }

    pub(crate) fn variant_fields_with_subst(
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
            | TypeKind::Literal(_)
            | TypeKind::This
            | TypeKind::Array(_)
            | TypeKind::Union(_)
            | TypeKind::Intersection(_)
            | TypeKind::Tuple(_)
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
            | TypeKind::TypePredicate { .. } => (None, Vec::new(), None),
        };

        let mut fields = bind.sum_variant_fields.get(variant).cloned();
        let mut def_bind: Option<std::sync::Arc<BindResult>> = None;
        if fields.is_none()
            && let Some(o) = &origin
        {
            let mb = self
                .resolver
                .module_bind(o.as_ref())
                .or_else(|| self.resolver.stdlib_bind(o.as_ref()));
            if let Some(mb) = mb {
                fields = mb.sum_variant_fields.get(variant).cloned();
                def_bind = Some(mb);
            }
        }
        let fields = fields?;

        let mut mapping = rustc_hash::FxHashMap::default();
        if !args.is_empty()
            && let Some(p) = &parent
        {
            let mut params = self.symbol_type_params_any(p.as_ref(), bind);
            if params.is_empty()
                && let Some(db) = &def_bind
            {
                params = db
                    .interner
                    .get(p.as_ref())
                    .and_then(|atom| db.scopes.get(db.global_scope).resolve(atom, &db.scopes))
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
            for (name, arg) in params.iter().zip(args.iter()) {
                mapping.insert(name.clone(), *arg);
            }
        }
        Some((fields, mapping))
    }
}

pub(crate) struct SequenceShape {
    pub fixed: Option<Vec<Type>>,
    pub element: Type,
    pub sequence: bool,
}

impl Checker<'_> {
    pub(crate) fn sequence_shape(
        &mut self,
        value_ty: &Type,
        tuple_pattern: bool,
        element_count: usize,
        has_rest: bool,
        range: varn_core::SourceRange,
        bind: &BindResult,
    ) -> SequenceShape {
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
        let value_is_array = matches!(self.ty_table.get(value_ty.0), varn_core::TypeKind::Array(_));
        let is_readonly_sequence = value_ty.sequence_element(&self.ty_table).is_some();
        let not_sequence = match self.ty_table.get(value_ty.0) {
            varn_core::TypeKind::Primitive(p) => !matches!(
                p,
                varn_core::LangPrimitive::Dynamic
                    | varn_core::LangPrimitive::Error
                    | varn_core::LangPrimitive::Str
            ),
            varn_core::TypeKind::Named(..)
            | varn_core::TypeKind::Generic(..)
            | varn_core::TypeKind::Record(_)
            | varn_core::TypeKind::Interface(_)
            | varn_core::TypeKind::Fn(_)
            | varn_core::TypeKind::EnumVariant { .. } => !is_readonly_sequence,
            varn_core::TypeKind::Literal(_)
            | varn_core::TypeKind::This
            | varn_core::TypeKind::Array(_)
            | varn_core::TypeKind::Union(_)
            | varn_core::TypeKind::Intersection(_)
            | varn_core::TypeKind::Tuple(_)
            | varn_core::TypeKind::TemplateLiteral(_)
            | varn_core::TypeKind::Typeof(_)
            | varn_core::TypeKind::KeyOf(_)
            | varn_core::TypeKind::IndexedAccess { .. }
            | varn_core::TypeKind::Mapped { .. }
            | varn_core::TypeKind::Conditional { .. }
            | varn_core::TypeKind::Infer(_)
            | varn_core::TypeKind::TypePredicate { .. } => false,
        };
        if not_sequence {
            self.emit(
                varn_core::Diagnostic::error(
                    varn_core::ErrorCode::TypeMismatch,
                    format!(
                        "'{}' is neither an array nor a tuple, so it cannot be destructured by position",
                        value_ty.display(&self.ty_table, &bind.interner)
                    ),
                )
                .with_range(range),
            );
            return SequenceShape {
                fixed,
                element: Type::Error,
                sequence: false,
            };
        }
        if tuple_pattern && value_is_array {
            self.emit(
                varn_core::Diagnostic::error(
                    varn_core::ErrorCode::TypeMismatch,
                    format!(
                        "'{}' is an array: destructure it with [a, b], not (a, b)",
                        value_ty.display(&self.ty_table, &bind.interner)
                    ),
                )
                .with_range(range),
            );
        } else if !tuple_pattern && fixed.is_some() {
            self.emit(
                varn_core::Diagnostic::error(
                    varn_core::ErrorCode::TypeMismatch,
                    format!(
                        "'{}' is a tuple: destructure it with (a, b), not [a, b]",
                        value_ty.display(&self.ty_table, &bind.interner)
                    ),
                )
                .with_range(range),
            );
        }
        let element = value_ty.get_array_element_type(&self.ty_table);
        if let Some(items) = &fixed {
            let arity_ok = if has_rest {
                element_count <= items.len()
            } else {
                element_count == items.len()
            };
            if !arity_ok {
                self.emit(
                    varn_core::Diagnostic::error(
                        varn_core::ErrorCode::TupleIndexOutOfBounds,
                        format!(
                            "this pattern destructures {} elements, but '{}' has {}",
                            element_count,
                            value_ty.display(&self.ty_table, &bind.interner),
                            items.len()
                        ),
                    )
                    .with_range(range),
                );
            }
        }
        SequenceShape {
            fixed,
            element,
            sequence: true,
        }
    }
}
