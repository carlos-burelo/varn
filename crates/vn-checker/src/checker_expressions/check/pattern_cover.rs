use crate::checker::Checker;
use vn_core::ast::{MatchPattern, VariantPayload};
use vn_core::TypeKind;
use vn_sem::bind::BindResult;
use vn_sem::types::Type;

pub(super) enum Cover {
    All,
    Variant(String),
    Exact(usize),
    AtLeast(usize),
    Nothing,
}

pub(super) fn lengths_covered(covers: &[Cover]) -> bool {
    let Some(open_from) = covers
        .iter()
        .filter_map(|c| match c {
            Cover::AtLeast(n) => Some(*n),
            Cover::All | Cover::Variant(_) | Cover::Exact(_) | Cover::Nothing => None,
        })
        .min()
    else {
        return covers.iter().any(|c| matches!(c, Cover::All));
    };
    (0..open_from).all(|len| {
        covers
            .iter()
            .any(|c| matches!(c, Cover::Exact(n) if *n == len))
    })
}

impl Checker<'_> {
    pub(super) fn type_variant_names(&self, ty: &Type, bind: &BindResult) -> Option<Vec<String>> {
        let (TypeKind::Named(name_atom, origin_atom)
        | TypeKind::Generic(name_atom, _, origin_atom)) = self.ty_table.get(ty.0)
        else {
            return None;
        };
        let name = self.resolve_bind_atom(bind, name_atom);
        let foreign = origin_atom.and_then(|o| {
            let origin = self.resolve_bind_atom(bind, o);
            self.resolver
                .module_bind(&origin)
                .or_else(|| self.resolver.stdlib_bind(&origin))
        });
        let owner: &BindResult = foreign.as_deref().unwrap_or(bind);
        if let Some(variants) = owner.sum_type_variants.get(name.as_ref()) {
            return Some(variants.iter().map(|v| v.to_string()).collect());
        }
        let members = owner.get_enum_members_local(name.as_ref())?;
        Some(
            members
                .iter()
                .filter(|m| {
                    owner
                        .sum_variant_parent
                        .get(m.name.as_ref())
                        .is_some_and(|parent| parent.as_ref() == name.as_ref())
                })
                .map(|m| m.name.to_string())
                .collect(),
        )
    }

    pub(super) fn irrefutable_for(
        &mut self,
        pattern: &MatchPattern,
        ty: &Type,
        bind: &BindResult,
    ) -> bool {
        match pattern {
            MatchPattern::Wildcard => true,
            MatchPattern::Identifier(name) => match self.type_variant_names(ty, bind) {
                Some(variants) => !variants.iter().any(|v| v == bind.interner.resolve(*name)),
                None => true,
            },
            MatchPattern::Tuple(items) => {
                if !ty.is_unresolved() && ty.is_nullable(&self.ty_table) {
                    return false;
                }
                let fixed: Option<Vec<Type>> =
                    if let TypeKind::Tuple(list) = self.ty_table.get(ty.0) {
                        Some(
                            self.ty_table
                                .get_list(list)
                                .iter()
                                .map(|id| Type::resolved(*id))
                                .collect(),
                        )
                    } else {
                        None
                    };
                match fixed {
                    Some(types) => {
                        types.len() == items.len()
                            && items
                                .iter()
                                .zip(types.iter())
                                .all(|(item, item_ty)| self.irrefutable_for(item, item_ty, bind))
                    }
                    None => {
                        ty.is_unresolved()
                            && items
                                .iter()
                                .all(|item| self.irrefutable_for(item, &Type::Dynamic, bind))
                    }
                }
            }
            MatchPattern::Record { fields, .. } => {
                if !ty.is_unresolved() && ty.is_nullable(&self.ty_table) {
                    return false;
                }
                for field in fields {
                    let Some(sub) = &field.pattern else { continue };
                    let name = bind.interner.resolve(field.name);
                    let member_ty = self
                        .find_member_info(ty, name, bind)
                        .map(|(found, _)| found)
                        .unwrap_or(Type::Dynamic);
                    if !self.irrefutable_for(sub, &member_ty, bind) {
                        return false;
                    }
                }
                true
            }
            MatchPattern::Literal(_)
            | MatchPattern::Array { .. }
            | MatchPattern::EnumVariant { .. } => false,
        }
    }

    pub(super) fn case_cover(
        &mut self,
        pattern: &MatchPattern,
        subject_ty: &Type,
        bind: &BindResult,
    ) -> Cover {
        match pattern {
            MatchPattern::Wildcard => Cover::All,
            MatchPattern::Identifier(name) => {
                let name = bind.interner.resolve(*name);
                match self.type_variant_names(subject_ty, bind) {
                    Some(variants) if variants.iter().any(|v| v == name) => {
                        Cover::Variant(name.to_owned())
                    }
                    Some(_) | None => Cover::All,
                }
            }
            MatchPattern::EnumVariant {
                variant_name,
                payload,
                ..
            } => {
                let full = bind.interner.resolve(*variant_name);
                let name = full.rsplit('.').next().unwrap_or(full).to_owned();
                if self.payload_irrefutable(&name, payload, subject_ty, bind) {
                    Cover::Variant(name)
                } else {
                    Cover::Nothing
                }
            }
            MatchPattern::Literal(expr) => {
                let named = vn_core::ast::literal_variant_name(self.ast_arena, *expr);
                match named {
                    Some(name) => Cover::Variant(bind.interner.resolve(name).to_owned()),
                    None => Cover::Nothing,
                }
            }
            MatchPattern::Array { elements, rest } => {
                let present = if subject_ty.is_unresolved() {
                    *subject_ty
                } else {
                    subject_ty.non_nullified(std::sync::Arc::make_mut(&mut self.ty_table))
                };
                let element = present.get_array_element_type(&self.ty_table);
                let all_open = elements
                    .iter()
                    .all(|item| self.irrefutable_for(item, &element, bind));
                match (all_open, rest) {
                    (false, _) => Cover::Nothing,
                    (true, vn_core::ast::SequenceRest::Closed) => Cover::Exact(elements.len()),
                    (
                        true,
                        vn_core::ast::SequenceRest::Open | vn_core::ast::SequenceRest::Bind(_),
                    ) => Cover::AtLeast(elements.len()),
                }
            }
            MatchPattern::Tuple(_) | MatchPattern::Record { .. } => Cover::Nothing,
        }
    }

    fn payload_irrefutable(
        &mut self,
        variant: &str,
        payload: &VariantPayload,
        subject_ty: &Type,
        bind: &BindResult,
    ) -> bool {
        let resolved = self.variant_fields_with_subst(variant, subject_ty, bind);
        let field_type = |checker: &mut Self, at: Option<usize>, name: Option<&str>| {
            let Some((fields, mapping)) = &resolved else {
                return Type::Dynamic;
            };
            let found = match (at, name) {
                (Some(i), _) => fields.get(i),
                (None, Some(n)) => fields.iter().find(|(fname, _)| fname.as_ref() == n),
                (None, None) => None,
            };
            match found {
                Some((_, ty)) if mapping.is_empty() => *ty,
                Some((_, ty)) => {
                    crate::generic_substitution::map_generics_cached(checker, ty, mapping)
                }
                None => Type::Dynamic,
            }
        };
        match payload {
            VariantPayload::Positional(items) => items.iter().enumerate().all(|(i, item)| {
                let ty = field_type(self, Some(i), None);
                self.irrefutable_for(item, &ty, bind)
            }),
            VariantPayload::Named { fields, .. } => fields.iter().all(|field| {
                let Some(sub) = &field.pattern else {
                    return true;
                };
                let ty = field_type(self, None, Some(bind.interner.resolve(field.name)));
                self.irrefutable_for(sub, &ty, bind)
            }),
        }
    }
}
