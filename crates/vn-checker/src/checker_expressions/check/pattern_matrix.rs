use crate::checker::Checker;
use vn_core::ast::{ExprKind, MatchCase, MatchPattern, VariantPayload};
use vn_core::TypeKind;
use vn_sem::bind::BindResult;
use vn_sem::types::Type;

const MAX_DEPTH: usize = 48;

#[derive(Clone, Copy)]
enum Cell<'p> {
    Wild,
    Pat(&'p MatchPattern),
}

enum Head<'p> {
    Wild,
    Variant(String, Vec<Cell<'p>>, Vec<(String, Cell<'p>)>),
    Tuple(Vec<Cell<'p>>),
    Null,
    Other,
}

impl Checker<'_> {
    pub(super) fn matrix_exhaustive(
        &mut self,
        cases: &[MatchCase],
        subject_ty: &Type,
        bind: &BindResult,
    ) -> bool {
        let rows: Vec<Vec<Cell>> = cases
            .iter()
            .filter(|c| c.guard.is_none())
            .map(|c| vec![Cell::Pat(&c.pattern)])
            .collect();
        !self.wildcard_useful(rows, vec![*subject_ty], bind, 0)
    }

    fn wildcard_useful(
        &mut self,
        rows: Vec<Vec<Cell>>,
        tys: Vec<Type>,
        bind: &BindResult,
        depth: usize,
    ) -> bool {
        if tys.is_empty() {
            return rows.is_empty();
        }
        if depth >= MAX_DEPTH {
            return true;
        }
        let ty = tys[0];
        let rest_tys: Vec<Type> = tys[1..].to_vec();
        let heads: Vec<Head> = rows.iter().map(|r| self.head_of(r[0], &ty, bind)).collect();
        let constructor_present = heads
            .iter()
            .any(|h| matches!(h, Head::Variant(..) | Head::Tuple(_) | Head::Null));

        if let TypeKind::Union(list) = self.ty_table.get(ty.0)
            && constructor_present
        {
            let members: Vec<Type> = self
                .ty_table
                .get_list(list)
                .iter()
                .map(|id| Type::resolved(*id))
                .collect();
            for member in members {
                let is_null = member == Type::Null;
                let mut spec: Vec<Vec<Cell>> = Vec::new();
                for (row, head) in rows.iter().zip(heads.iter()) {
                    match (is_null, head) {
                        (true, Head::Wild | Head::Null) => spec.push(row[1..].to_vec()),
                        (false, Head::Null) => {}
                        (false, _) => spec.push(row.clone()),
                        (true, Head::Variant(..) | Head::Tuple(_) | Head::Other) => {}
                    }
                }
                let mut next_tys: Vec<Type> = Vec::new();
                if !is_null {
                    next_tys.push(member);
                }
                next_tys.extend(rest_tys.iter().copied());
                if self.wildcard_useful(spec, next_tys, bind, depth + 1) {
                    return true;
                }
            }
            return false;
        }

        if !constructor_present {
            let default_rows: Vec<Vec<Cell>> = rows
                .iter()
                .zip(heads.iter())
                .filter(|(_, h)| matches!(h, Head::Wild))
                .map(|(r, _)| r[1..].to_vec())
                .collect();
            return self.wildcard_useful(default_rows, rest_tys, bind, depth + 1);
        }

        if let Some(variants) = self.type_variant_names(&ty, bind) {
            for variant in variants {
                let Some(payload_tys) = self.variant_payload_types(&variant, &ty, bind) else {
                    return true;
                };
                let Some(names) = self.variant_payload_names(&variant, &ty, bind) else {
                    return true;
                };
                let mut spec: Vec<Vec<Cell>> = Vec::new();
                for (row, head) in rows.iter().zip(heads.iter()) {
                    let mut cells: Vec<Cell> = match head {
                        Head::Wild => vec![Cell::Wild; payload_tys.len()],
                        Head::Variant(name, positional, named) if *name == variant => {
                            let mut cells = vec![Cell::Wild; payload_tys.len()];
                            for (at, cell) in positional.iter().enumerate() {
                                if at < cells.len() {
                                    cells[at] = *cell;
                                }
                            }
                            for (field, cell) in named {
                                if let Some(at) = names.iter().position(|n| n == field) {
                                    cells[at] = *cell;
                                }
                            }
                            cells
                        }
                        Head::Variant(..) | Head::Tuple(_) | Head::Null | Head::Other => continue,
                    };
                    cells.extend_from_slice(&row[1..]);
                    spec.push(cells);
                }
                let mut next_tys = payload_tys;
                next_tys.extend(rest_tys.iter().copied());
                if self.wildcard_useful(spec, next_tys, bind, depth + 1) {
                    return true;
                }
            }
            return false;
        }

        if let TypeKind::Tuple(list) = self.ty_table.get(ty.0) {
            let elems: Vec<Type> = self
                .ty_table
                .get_list(list)
                .iter()
                .map(|id| Type::resolved(*id))
                .collect();
            let mut spec: Vec<Vec<Cell>> = Vec::new();
            for (row, head) in rows.iter().zip(heads.iter()) {
                let mut cells: Vec<Cell> = match head {
                    Head::Wild => vec![Cell::Wild; elems.len()],
                    Head::Tuple(items) if items.len() == elems.len() => items.clone(),
                    Head::Tuple(_) | Head::Variant(..) | Head::Null | Head::Other => continue,
                };
                cells.extend_from_slice(&row[1..]);
                spec.push(cells);
            }
            let mut next_tys = elems;
            next_tys.extend(rest_tys.iter().copied());
            return self.wildcard_useful(spec, next_tys, bind, depth + 1);
        }

        true
    }

    fn head_of<'p>(&mut self, cell: Cell<'p>, ty: &Type, bind: &BindResult) -> Head<'p> {
        let Cell::Pat(pattern) = cell else {
            return Head::Wild;
        };
        match pattern {
            MatchPattern::Wildcard => Head::Wild,
            MatchPattern::Identifier(name) => {
                let text = bind.interner.resolve(*name);
                match self.type_variant_names(ty, bind) {
                    Some(variants) if variants.iter().any(|v| v == text) => {
                        Head::Variant(text.to_owned(), Vec::new(), Vec::new())
                    }
                    Some(_) | None => Head::Wild,
                }
            }
            MatchPattern::EnumVariant {
                variant_name,
                payload,
                ..
            } => {
                let full = bind.interner.resolve(*variant_name);
                let name = full.rsplit('.').next().unwrap_or(full).to_owned();
                match payload {
                    VariantPayload::Positional(items) => {
                        Head::Variant(name, items.iter().map(Cell::Pat).collect(), Vec::new())
                    }
                    VariantPayload::Named { fields, .. } => Head::Variant(
                        name,
                        Vec::new(),
                        fields
                            .iter()
                            .map(|f| {
                                let cell = f.pattern.as_ref().map_or(Cell::Wild, Cell::Pat);
                                (bind.interner.resolve(f.name).to_owned(), cell)
                            })
                            .collect(),
                    ),
                }
            }
            MatchPattern::Literal(expr) => {
                let arena = self.ast_arena;
                let named = vn_core::ast::literal_variant_name(arena, *expr);
                if matches!(arena.expr(*expr).kind, ExprKind::NullLiteral) {
                    return Head::Null;
                }
                match named {
                    Some(name) if self.type_variant_names(ty, bind).is_some() => Head::Variant(
                        bind.interner.resolve(name).to_owned(),
                        Vec::new(),
                        Vec::new(),
                    ),
                    Some(_) | None => Head::Other,
                }
            }
            MatchPattern::Tuple(items) => Head::Tuple(items.iter().map(Cell::Pat).collect()),
            MatchPattern::Record { .. } if self.irrefutable_for(pattern, ty, bind) => Head::Wild,
            MatchPattern::Array { .. } | MatchPattern::Record { .. } => Head::Other,
        }
    }

    fn variant_payload_types(
        &mut self,
        variant: &str,
        ty: &Type,
        bind: &BindResult,
    ) -> Option<Vec<Type>> {
        let (fields, mapping) = self.variant_fields_with_subst(variant, ty, bind)?;
        Some(
            fields
                .iter()
                .map(|(_, field_ty)| {
                    if mapping.is_empty() {
                        *field_ty
                    } else {
                        crate::generic_substitution::map_generics_cached(self, field_ty, &mapping)
                    }
                })
                .collect(),
        )
    }

    fn variant_payload_names(
        &mut self,
        variant: &str,
        ty: &Type,
        bind: &BindResult,
    ) -> Option<Vec<String>> {
        let (fields, _) = self.variant_fields_with_subst(variant, ty, bind)?;
        Some(fields.iter().map(|(name, _)| name.to_string()).collect())
    }
}
