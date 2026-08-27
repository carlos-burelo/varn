use crate::binder::BindResult;
use crate::checker::Checker;
use crate::symbol::SymbolId;
use crate::types::{ObjectTypeMember, Type};
use rustc_hash::FxHashMap;
use varn_core::ast::operators::{BinaryOp, UnaryOp};
use varn_core::ast::{Expr, ExprKind};
use varn_core::TypeKind;

impl<'r> Checker<'r> {
    pub(crate) fn can_extract_narrowings(&self, expr: &Expr) -> bool {
        matches!(
            &expr.kind,
            ExprKind::Binary {
                op: BinaryOp::Eq | BinaryOp::NotEq | BinaryOp::Instanceof,
                ..
            } | ExprKind::Logical { .. }
                | ExprKind::Is { .. }
                | ExprKind::Call { .. }
                | ExprKind::Identifier { .. }
                | ExprKind::Unary {
                    op: UnaryOp::Not,
                    ..
                }
        )
    }

    pub(crate) fn extract_narrowings(
        &mut self,
        expr: &Expr,
        bind: &BindResult,
        is_true_branch: bool,
    ) -> Vec<(crate::symbol::SymbolId, Type)> {
        let cache_key = (expr.id, is_true_branch, self.current_scope);
        if let Some(cached) = self.narrowings_cache.get(&cache_key) {
            return cached.clone();
        }

        let mut narrowings = Vec::new();

        match &expr.kind {
            ExprKind::Unary {
                op: UnaryOp::Not,
                operand,
                ..
            } => {
                narrowings.extend(self.extract_narrowings(operand, bind, !is_true_branch));
            }

            ExprKind::Identifier { name } => {
                let scope = bind.scopes.get(self.current_scope);
                if let Some(id) = scope.resolve(name, &bind.scopes) {
                    let original_ty = self
                        .symbol_types
                        .get(&id)
                        .cloned()
                        .or_else(|| bind.arena.get(id).ty.clone());
                    if let Some(ty) = original_ty {
                        if is_true_branch {
                            let narrowed = ty.non_nullified();
                            if !narrowed.is_dynamic() && narrowed != ty {
                                narrowings.push((id, narrowed));
                            }
                        } else if ty.is_nullable() {
                            narrowings.push((id, Type::Null));
                        }
                    }
                }
            }

            ExprKind::Binary { left, right, op } => {
                let is_eq = *op == BinaryOp::Eq;
                let is_neq = *op == BinaryOp::NotEq;

                // 1. typeof x === "str" / "int" / "float" / "bool" / etc.
                let typeof_check = match (&left.kind, &right.kind) {
                    (
                        ExprKind::Unary {
                            op: UnaryOp::Typeof,
                            operand: typeof_op,
                            ..
                        },
                        ExprKind::StrLiteral { value },
                    ) => Some((typeof_op, value.as_ref())),
                    (
                        ExprKind::StrLiteral { value },
                        ExprKind::Unary {
                            op: UnaryOp::Typeof,
                            operand: typeof_op,
                            ..
                        },
                    ) => Some((typeof_op, value.as_ref())),
                    _ => None,
                };

                if let Some((typeof_op, value)) = typeof_check {
                    if (is_eq && is_true_branch) || (is_neq && !is_true_branch) {
                        if let ExprKind::Identifier { name } = &typeof_op.kind {
                            let scope = bind.scopes.get(self.current_scope);
                            if let Some(id) = scope.resolve(name, &bind.scopes) {
                                let narrowed_ty = crate::binder::resolve_primitive(
                                    value,
                                    Some(&crate::binder::BindView::new(bind, self.resolver)),
                                );
                                narrowings.push((id, narrowed_ty));
                            }
                        }
                    }
                }

                // 2. x !== null / null !== x / x === null / null === x
                let (ident_name, is_null_check) = match (&left.kind, &right.kind) {
                    (ExprKind::Identifier { name }, ExprKind::NullLiteral) => (Some(name), true),
                    (ExprKind::NullLiteral, ExprKind::Identifier { name }) => (Some(name), true),
                    _ => (None, false),
                };

                if is_null_check {
                    if let Some(name) = ident_name {
                        let scope = bind.scopes.get(self.current_scope);
                        if let Some(id) = scope.resolve(name, &bind.scopes) {
                            if (is_neq && is_true_branch) || (is_eq && !is_true_branch) {
                                let original_ty = self
                                    .symbol_types
                                    .get(&id)
                                    .cloned()
                                    .or_else(|| bind.arena.get(id).ty.clone());
                                if let Some(ty) = original_ty {
                                    let narrowed = ty.non_nullified();
                                    if !narrowed.is_dynamic() {
                                        narrowings.push((id, narrowed));
                                    }
                                }
                            } else if &**name != "_" && &**name != "__variant__" {
                                narrowings.push((id, Type::Null));
                            }
                        }
                    }
                }

                // 3. Discriminated union property check: obj.kind === "foo"
                if is_eq || is_neq {
                    if let ExprKind::Member {
                        object,
                        property,
                        computed: false,
                        ..
                    } = &left.kind
                    {
                        if let (
                            ExprKind::Identifier { name: obj_name },
                            ExprKind::Identifier { name: prop_name },
                        ) = (&object.kind, &property.kind)
                        {
                            let disc_ty: Option<Type> = match &right.kind {
                                ExprKind::StrLiteral { .. } => Some(Type::Str),
                                ExprKind::IntLiteral { .. } => Some(Type::Int),
                                _ => None,
                            };
                            if let Some(disc_ty) = disc_ty {
                                let scope = bind.scopes.get(self.current_scope);
                                if let Some(id) = scope.resolve(obj_name, &bind.scopes) {
                                    let original_ty = bind.arena.get(id).ty.clone();
                                    if let Some(Type(TypeKind::Union(members), _)) = &original_ty {
                                        let mut matched: Vec<Type> = Vec::new();
                                        let mut unmatched: Vec<Type> = Vec::new();
                                        for m in members.iter() {
                                            let hits = match &m.0 {
                                                TypeKind::Object(fields) => {
                                                    fields.iter().any(|f| match f {
                                                        ObjectTypeMember::Property {
                                                            name,
                                                            ty,
                                                            ..
                                                        } => {
                                                            name.as_ref() == prop_name.as_ref()
                                                                && ty == &disc_ty
                                                        }
                                                        _ => false,
                                                    })
                                                }
                                                TypeKind::Named(cn, _) => bind
                                                    .get_interface_members_local(cn.as_ref())
                                                    .or_else(|| {
                                                        bind.get_class_entry(cn.as_ref())
                                                            .map(|e| &e.members)
                                                    })
                                                    .is_some_and(|ms| {
                                                        ms.iter().any(|cm| {
                                                            cm.name.as_ref() == prop_name.as_ref()
                                                                && cm.ty == disc_ty
                                                        })
                                                    }),
                                                _ => false,
                                            };
                                            if hits {
                                                matched.push(m.clone());
                                            } else {
                                                unmatched.push(m.clone());
                                            }
                                        }
                                        let make_ty = |v: Vec<Type>| match v.len() {
                                            0 => None,
                                            1 => Some(v.into_iter().next().unwrap()),
                                            _ => Some(Type::union(v)),
                                        };
                                        if (is_eq && is_true_branch) || (is_neq && !is_true_branch)
                                        {
                                            if !matched.is_empty() {
                                                if let Some(t) = make_ty(matched) {
                                                    narrowings.push((id, t));
                                                }
                                            }
                                        } else if (is_neq && is_true_branch)
                                            || (is_eq && !is_true_branch)
                                        {
                                            if !matched.is_empty() {
                                                if let Some(t) = make_ty(unmatched) {
                                                    narrowings.push((id, t));
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // 4. Instanceof narrowing: x instanceof User
                if *op == BinaryOp::Instanceof {
                    if let (
                        ExprKind::Identifier { name },
                        ExprKind::Identifier { name: class_name },
                    ) = (&left.kind, &right.kind)
                    {
                        let scope = bind.scopes.get(self.current_scope);
                        if let Some(id) = scope.resolve(name, &bind.scopes) {
                            if is_true_branch {
                                narrowings.push((id, Type::named(class_name.clone())));
                            } else if let Some(ty) = &bind.arena.get(id).ty {
                                let narrowed = ty.minus_named(class_name.as_ref());
                                if &**name == "_" {
                                    narrowings.push((id, narrowed));
                                }
                            }
                        }
                    }
                }
            }

            ExprKind::Logical {
                left,
                right,
                op: varn_core::ast::operators::LogicalOp::And,
            } => {
                if is_true_branch {
                    narrowings.extend(self.extract_narrowings(left, bind, true));
                    narrowings.extend(self.extract_narrowings(right, bind, true));
                } else {
                    let left_n = self.extract_narrowings(left, bind, false);
                    let right_n = self.extract_narrowings(right, bind, false);
                    narrowings.extend(self.merge_narrowings(left_n, right_n, false));
                }
            }

            ExprKind::Logical {
                left,
                right,
                op: varn_core::ast::operators::LogicalOp::Or,
            } => {
                if is_true_branch {
                    let left_n = self.extract_narrowings(left, bind, true);
                    let right_n = self.extract_narrowings(right, bind, true);
                    narrowings.extend(self.merge_narrowings(left_n, right_n, true));
                } else {
                    narrowings.extend(self.extract_narrowings(left, bind, false));
                    narrowings.extend(self.extract_narrowings(right, bind, false));
                }
            }

            ExprKind::Is {
                expression,
                type_ann,
            } => {
                if let ExprKind::Identifier { name: arg_name } = &expression.kind {
                    let scope = bind.scopes.get(self.current_scope);
                    if let Some(id) = scope.resolve(arg_name, &bind.scopes) {
                        if is_true_branch {
                            let narrowed_ty =
                                crate::binder::resolve_type_node(type_ann, Some(&crate::binder::BindView::new(bind, self.resolver)));
                            narrowings.push((id, narrowed_ty));
                        } else {
                            if let Some(original_ty) = &bind.arena.get(id).ty {
                                let target_ty =
                                    crate::binder::resolve_type_node(type_ann, Some(&crate::binder::BindView::new(bind, self.resolver)));
                                let narrowed = original_ty.minus(&target_ty);
                                if narrowed != *original_ty {
                                    narrowings.push((id, narrowed));
                                }
                            }
                        }
                    }
                }
            }

            ExprKind::Call { callee, args, .. } => {
                let callee_ty = self.infer_type(callee, bind).non_nullified();
                if let TypeKind::Fn(ft) = &callee_ty.0 {
                    if let TypeKind::TypePredicate {
                        parameter_name,
                        target_type,
                    } = &ft.return_type.0
                    {
                        let arg_expr = if let Some(pos) = ft
                            .params
                            .iter()
                            .position(|p| p.name.as_deref() == Some(parameter_name.as_ref()))
                        {
                            args.get(pos).and_then(|a| match a {
                                varn_core::ast::Arg::Positional(e) => Some(e),
                                _ => None,
                            })
                        } else if args.len() == 1 {
                            match &args[0] {
                                varn_core::ast::Arg::Positional(e) => Some(e),
                                _ => None,
                            }
                        } else {
                            None
                        };

                        if let Some(ExprKind::Identifier { name: arg_name }) = arg_expr.map(|e| &e.kind) {
                            let scope = bind.scopes.get(self.current_scope);
                            if let Some(id) = scope.resolve(arg_name, &bind.scopes) {
                                if is_true_branch {
                                    narrowings.push((id, (**target_type).clone()));
                                } else if let Some(original_ty) = &bind.arena.get(id).ty {
                                    let narrowed = original_ty.minus(target_type);
                                    if narrowed != *original_ty {
                                        narrowings.push((id, narrowed));
                                    }
                                }
                            }
                        }
                    }
                }
            }

            _ => {}
        }
        self.narrowings_cache.insert(cache_key, narrowings.clone());
        narrowings
    }

    pub(crate) fn collect_match_disc_narrowings(
        &self,
        subject: &Expr,
        bind: &BindResult,
    ) -> Option<(SymbolId, Vec<Type>)> {
        if let ExprKind::Member {
            object,
            property,
            computed: false,
            ..
        } = &subject.kind
        {
            if let (
                ExprKind::Identifier { name: obj_name },
                ExprKind::Identifier { name: _prop_name },
            ) = (&object.kind, &property.kind)
            {
                let scope = bind.scopes.get(self.current_scope);
                if let Some(id) = scope.resolve(obj_name, &bind.scopes) {
                    if let Some(Type(TypeKind::Union(members), _)) = &bind.arena.get(id).ty {
                        return Some((id, members.clone()));
                    }
                }
            }
        }
        None
    }

    pub(crate) fn union_member_matches_disc(
        &self,
        m: &Type,
        disc_ty: Option<&Type>,
        subject: Option<&Expr>,
        bind: &BindResult,
    ) -> bool {
        let Some(disc_ty) = disc_ty else { return false };
        let Some(subject) = subject else { return false };
        let ExprKind::Member {
            property,
            computed: false,
            ..
        } = &subject.kind
        else {
            return false;
        };
        let ExprKind::Identifier { name: prop_name } = &property.kind else {
            return false;
        };

        match &m.0 {
            TypeKind::Object(fields) => fields.iter().any(|f| match f {
                ObjectTypeMember::Property { name, ty, .. } => {
                    name.as_ref() == prop_name.as_ref() && ty == disc_ty
                }
                _ => false,
            }),
            TypeKind::Named(cn, _) => bind
                .get_interface_members_local(cn.as_ref())
                .or_else(|| bind.get_class_entry(cn.as_ref()).map(|e| &e.members))
                .is_some_and(|ms| {
                    ms.iter().any(|cm| {
                        cm.name.as_ref() == prop_name.as_ref() && cm.ty == *disc_ty
                    })
                }),
            _ => false,
        }
    }

    fn merge_narrowings(
        &self,
        left: Vec<(SymbolId, Type)>,
        right: Vec<(SymbolId, Type)>,
        is_union: bool,
    ) -> Vec<(SymbolId, Type)> {
        let mut map: FxHashMap<SymbolId, Vec<Type>> = FxHashMap::default();
        for (id, ty) in left {
            map.entry(id).or_default().push(ty);
        }
        for (id, ty) in right {
            map.entry(id).or_default().push(ty);
        }

        let mut merged = Vec::new();
        for (id, types) in map {
            if types.len() == 1 {
                if !is_union {
                    merged.push((id, types[0].clone()));
                }
            } else if is_union {
                merged.push((id, Type::union(types)));
            } else {
                merged.push((id, Type(TypeKind::Intersection(types), false)));
            }
        }
        merged
    }
}
