use super::recorder::Recorder;
use super::Checker;
use varn_core::ast::{ArrayEl, ExprId, ExprKind, ObjectProp, PropKey};
use varn_core::TypeKind;
use varn_sem::bind::BindResult;
use varn_sem::types::{ObjectTypeMember, Type};

const MAX_DEPTH: u8 = 24;

impl Checker<'_> {
    pub(crate) fn value_fits(
        &mut self,
        rec: &mut Recorder,
        target: &Type,
        given: &Type,
        expr: Option<ExprId>,
        bind: Option<&BindResult>,
    ) -> bool {
        if self.value_assignable_to(target, given, expr, bind) {
            return true;
        }
        match (expr, bind) {
            (Some(expr), Some(bind)) => {
                self.literal_fits(rec, target, expr, bind, 0)
                    || self.variant_fits(rec, target, expr, bind, 0)
            }
            (Some(_), None) | (None, _) => false,
        }
    }

    fn literal_fits(
        &mut self,
        rec: &mut Recorder,
        target: &Type,
        expr: ExprId,
        bind: &BindResult,
        depth: u8,
    ) -> bool {
        if depth > MAX_DEPTH {
            return false;
        }
        let arena = self.ast_arena;
        let literal = match &arena.expr(expr).kind {
            ExprKind::Paren { expression } => {
                return self.literal_fits(rec, target, *expression, bind, depth + 1)
            }
            ExprKind::Array { .. } | ExprKind::Object { .. } | ExprKind::Tuple { .. } => expr,
            ExprKind::IntLiteral { .. }
            | ExprKind::FloatLiteral { .. }
            | ExprKind::BigIntLiteral { .. }
            | ExprKind::DecimalLiteral { .. }
            | ExprKind::StrLiteral { .. }
            | ExprKind::CharLiteral { .. }
            | ExprKind::BoolLiteral { .. }
            | ExprKind::NullLiteral
            | ExprKind::Template { .. }
            | ExprKind::TaggedTemplate { .. }
            | ExprKind::Identifier { .. }
            | ExprKind::Missing
            | ExprKind::This
            | ExprKind::ImportMeta(_)
            | ExprKind::Super
            | ExprKind::Unary { .. }
            | ExprKind::Update { .. }
            | ExprKind::Binary { .. }
            | ExprKind::Logical { .. }
            | ExprKind::Assign { .. }
            | ExprKind::Conditional { .. }
            | ExprKind::Member { .. }
            | ExprKind::Call { .. }
            | ExprKind::New { .. }
            | ExprKind::Function { .. }
            | ExprKind::Arrow { .. }
            | ExprKind::Sequence { .. }
            | ExprKind::Await { .. }
            | ExprKind::Spawn { .. }
            | ExprKind::Yield { .. }
            | ExprKind::Spread { .. }
            | ExprKind::Pipeline { .. }
            | ExprKind::Range { .. }
            | ExprKind::NonNull { .. }
            | ExprKind::Try { .. }
            | ExprKind::As { .. }
            | ExprKind::Satisfies { .. }
            | ExprKind::ClassExpr { .. }
            | ExprKind::Match { .. }
            | ExprKind::Is { .. }
            | ExprKind::With { .. }
            | ExprKind::MetaAccess { .. } => return false,
        };
        let shapes = self.shapes_of(target, bind);
        shapes
            .into_iter()
            .any(|shape| self.literal_fits_shape(rec, &shape, literal, bind, depth))
    }

    fn shapes_of(&mut self, target: &Type, bind: &BindResult) -> Vec<Type> {
        use varn_sem::bind::TypeContext;
        match self.ty_table.get(target.0) {
            TypeKind::Union(members) => {
                let members: Vec<Type> = self
                    .ty_table
                    .get_list(members)
                    .iter()
                    .map(|m| Type::resolved(*m))
                    .collect();
                members
                    .iter()
                    .flat_map(|m| self.shapes_of(m, bind))
                    .collect()
            }
            TypeKind::Named(name, origin) => {
                let name_s = self.resolve_bind_atom(bind, name);
                let origin_s = origin.map(|o| self.resolve_bind_atom(bind, o));
                let alias = varn_sem::bind::BindView::new(bind, self.resolver)
                    .resolve_type_alias(&name_s, origin_s.as_deref())
                    .filter(|expanded| expanded.0 != target.0);
                match alias {
                    Some(expanded) => self.shapes_of(&expanded, bind),
                    None => vec![*target],
                }
            }
            TypeKind::Primitive(_)
            | TypeKind::Literal(_)
            | TypeKind::This
            | TypeKind::Array(_)
            | TypeKind::Intersection(_)
            | TypeKind::Tuple(_)
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
            | TypeKind::TypePredicate { .. } => vec![*target],
        }
    }

    fn literal_fits_shape(
        &mut self,
        rec: &mut Recorder,
        shape: &Type,
        literal: ExprId,
        bind: &BindResult,
        depth: u8,
    ) -> bool {
        let arena = self.ast_arena;
        let shape_kind = self.ty_table.get(shape.0);
        if let ExprKind::Array { elements } = &arena.expr(literal).kind {
            if !matches!(shape_kind, TypeKind::Array(_) | TypeKind::Generic(..)) {
                return false;
            }
            {
                let item = shape.get_array_element_type(&self.ty_table);
                if item.is_unresolved() && !matches!(shape_kind, TypeKind::Array(_)) {
                    return false;
                }
                return elements.iter().all(|el| match el {
                    ArrayEl::Expr(e) => self.part_fits(rec, &item, *e, bind, depth),
                    ArrayEl::Spread(e) => self.spread_fits(rec, &item, *e, bind),
                    ArrayEl::Hole => true,
                });
            }
        }
        if let ExprKind::Tuple { elements } = &arena.expr(literal).kind {
            let TypeKind::Tuple(list) = shape_kind else {
                return false;
            };
            let slots: Vec<Type> = self
                .ty_table
                .get_list(list)
                .iter()
                .map(|id| Type::resolved(*id))
                .collect();
            return slots.len() == elements.len()
                && elements
                    .iter()
                    .zip(&slots)
                    .all(|(element, slot)| self.part_fits(rec, slot, *element, bind, depth));
        }
        if let (ExprKind::Object { properties }, TypeKind::Record(mid) | TypeKind::Interface(mid)) =
            (&arena.expr(literal).kind, shape_kind)
        {
            {
                let members = self.ty_table.get_object_members(mid).to_vec();
                let mut present: Vec<&str> = Vec::with_capacity(properties.len());
                for prop in properties {
                    let ObjectProp::Property {
                        key: PropKey::Identifier(key) | PropKey::Str(key),
                        value,
                        ..
                    } = prop
                    else {
                        return false;
                    };
                    let slot = members.iter().find_map(|m| match m {
                        ObjectTypeMember::Property { name, ty, .. } if name.as_ref() == key => {
                            Some(Type::resolved(*ty))
                        }
                        ObjectTypeMember::Property { .. }
                        | ObjectTypeMember::Method { .. }
                        | ObjectTypeMember::Index { .. }
                        | ObjectTypeMember::Callable { .. } => None,
                    });
                    let slot = slot.or_else(|| {
                        members.iter().find_map(|m| match m {
                            ObjectTypeMember::Index { value_ty, .. } => {
                                Some(Type::resolved(*value_ty))
                            }
                            ObjectTypeMember::Property { .. }
                            | ObjectTypeMember::Method { .. }
                            | ObjectTypeMember::Callable { .. } => None,
                        })
                    });
                    let Some(slot) = slot else {
                        return false;
                    };
                    if !self.part_fits(rec, &slot, *value, bind, depth) {
                        return false;
                    }
                    present.push(key.as_str());
                }
                return members.iter().all(|m| match m {
                    ObjectTypeMember::Property {
                        name,
                        optional: false,
                        ..
                    }
                    | ObjectTypeMember::Method {
                        name,
                        optional: false,
                        ..
                    } => present.contains(&name.as_ref()),
                    ObjectTypeMember::Property { .. }
                    | ObjectTypeMember::Method { .. }
                    | ObjectTypeMember::Index { .. }
                    | ObjectTypeMember::Callable { .. } => true,
                });
            }
        }
        if let (ExprKind::Object { properties }, TypeKind::Named(..) | TypeKind::Generic(..)) =
            (&arena.expr(literal).kind, shape_kind)
        {
            let mut filled: Vec<ObjectTypeMember> = Vec::with_capacity(properties.len());
            for prop in properties {
                let ObjectProp::Property {
                    key: PropKey::Identifier(key) | PropKey::Str(key),
                    value,
                    ..
                } = prop
                else {
                    return false;
                };
                let slot = match self.find_member_info(shape, key, bind) {
                    Some((slot, _)) => {
                        if !self.part_fits(rec, &slot, *value, bind, depth) {
                            return false;
                        }
                        slot
                    }
                    None => self.infer_type(rec, *value, bind),
                };
                filled.push(ObjectTypeMember::Property {
                    name: std::sync::Arc::from(key.as_str()),
                    ty: slot.0,
                    optional: false,
                    readonly: false,
                });
            }
            let as_declared =
                Type::record(filled, &mut *std::sync::Arc::make_mut(&mut self.ty_table));
            return self.types_compatible_cached(shape, &as_declared, Some(bind));
        }
        false
    }

    fn part_fits(
        &mut self,
        rec: &mut Recorder,
        slot: &Type,
        part: ExprId,
        bind: &BindResult,
        depth: u8,
    ) -> bool {
        let given = self.infer_type(rec, part, bind);
        self.value_assignable_to(slot, &given, Some(part), Some(bind))
            || self.literal_fits(rec, slot, part, bind, depth + 1)
    }

    fn spread_fits(
        &mut self,
        rec: &mut Recorder,
        item: &Type,
        spread: ExprId,
        bind: &BindResult,
    ) -> bool {
        let given = self.infer_type(rec, spread, bind);
        if given.is_unresolved() {
            return true;
        }
        let parts: Vec<Type> = match self.ty_table.get(given.0) {
            TypeKind::Tuple(items) => self
                .ty_table
                .get_list(items)
                .iter()
                .map(|i| Type::resolved(*i))
                .collect(),
            TypeKind::Array(inner) => vec![Type::resolved(inner)],
            TypeKind::Primitive(_)
            | TypeKind::Literal(_)
            | TypeKind::This
            | TypeKind::Union(_)
            | TypeKind::Intersection(_)
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
            | TypeKind::TypePredicate { .. } => return false,
        };
        parts
            .iter()
            .all(|part| self.types_compatible_cached(item, part, Some(bind)))
    }
}

impl Checker<'_> {
    fn variant_fits(
        &mut self,
        rec: &mut Recorder,
        target: &Type,
        expr: ExprId,
        bind: &BindResult,
        depth: u8,
    ) -> bool {
        if depth > MAX_DEPTH {
            return false;
        }
        let arena = self.ast_arena;
        match &arena.expr(expr).kind {
            ExprKind::Paren { expression } => {
                self.variant_fits(rec, target, *expression, bind, depth + 1)
            }
            ExprKind::Conditional {
                consequent,
                alternate,
                ..
            } => {
                self.variant_fits(rec, target, *consequent, bind, depth + 1)
                    && self.variant_fits(rec, target, *alternate, bind, depth + 1)
            }
            ExprKind::Member { .. } | ExprKind::Call { .. } => {
                if !self.is_variant_construction(expr, bind) {
                    return false;
                }
                let given = self.infer_type(rec, expr, bind);
                self.unresolved_arguments_fit(target, &given)
            }
            ExprKind::IntLiteral { .. }
            | ExprKind::FloatLiteral { .. }
            | ExprKind::BigIntLiteral { .. }
            | ExprKind::DecimalLiteral { .. }
            | ExprKind::StrLiteral { .. }
            | ExprKind::CharLiteral { .. }
            | ExprKind::BoolLiteral { .. }
            | ExprKind::NullLiteral
            | ExprKind::Template { .. }
            | ExprKind::TaggedTemplate { .. }
            | ExprKind::Identifier { .. }
            | ExprKind::Missing
            | ExprKind::This
            | ExprKind::ImportMeta(_)
            | ExprKind::Super
            | ExprKind::Array { .. }
            | ExprKind::Object { .. }
            | ExprKind::Tuple { .. }
            | ExprKind::Unary { .. }
            | ExprKind::Update { .. }
            | ExprKind::Binary { .. }
            | ExprKind::Logical { .. }
            | ExprKind::Assign { .. }
            | ExprKind::New { .. }
            | ExprKind::Function { .. }
            | ExprKind::Arrow { .. }
            | ExprKind::Sequence { .. }
            | ExprKind::Await { .. }
            | ExprKind::Spawn { .. }
            | ExprKind::Yield { .. }
            | ExprKind::Spread { .. }
            | ExprKind::Pipeline { .. }
            | ExprKind::Range { .. }
            | ExprKind::NonNull { .. }
            | ExprKind::Try { .. }
            | ExprKind::As { .. }
            | ExprKind::Satisfies { .. }
            | ExprKind::ClassExpr { .. }
            | ExprKind::Match { .. }
            | ExprKind::Is { .. }
            | ExprKind::With { .. }
            | ExprKind::MetaAccess { .. } => false,
        }
    }

    fn is_variant_construction(&self, expr: ExprId, bind: &BindResult) -> bool {
        let arena = self.ast_arena;
        let member = if let ExprKind::Call { callee, .. } = &arena.expr(expr).kind {
            *callee
        } else {
            expr
        };
        let ExprKind::Member { object, .. } = &arena.expr(member).kind else {
            return false;
        };
        let ExprKind::Identifier { name } = &arena.expr(*object).kind else {
            return false;
        };
        bind.scopes
            .get(self.current_scope)
            .resolve(*name, &bind.scopes)
            .is_some_and(|sid| bind.arena.get(sid).kind == varn_sem::symbol::SymbolKind::Enum)
    }

    fn unresolved_arguments_fit(&mut self, target: &Type, given: &Type) -> bool {
        let target = target.non_nullified(&mut *std::sync::Arc::make_mut(&mut self.ty_table));
        let (
            TypeKind::Generic(target_name, target_args, target_origin),
            TypeKind::Generic(given_name, given_args, given_origin),
        ) = (self.ty_table.get(target.0), self.ty_table.get(given.0))
        else {
            return false;
        };
        let target_args = self.ty_table.get_list(target_args).to_vec();
        let given_args = self.ty_table.get_list(given_args).to_vec();
        target_name == given_name
            && target_origin == given_origin
            && target_args.len() == given_args.len()
            && target_args
                .iter()
                .zip(&given_args)
                .all(|(wanted, got)| wanted == got || *got == Type::Never.0)
    }
}
