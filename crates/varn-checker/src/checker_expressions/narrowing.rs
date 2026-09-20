use crate::binder::BindResult;
use crate::checker::Checker;
use crate::symbol::SymbolId;
use crate::types::{ObjectTypeMember, Type};
use rustc_hash::FxHashMap;
use varn_core::ast::operators::{BinaryOp, UnaryOp};
use varn_core::ast::{ExprId, ExprKind};
use varn_core::TypeKind;

impl<'r> Checker<'r> {
    pub(crate) fn can_extract_narrowings(&self, expr: ExprId) -> bool {
        matches!(
            &self.ast_arena.expr(expr).kind,
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
        expr: ExprId,
        bind: &BindResult,
        is_true_branch: bool,
    ) -> Vec<(crate::symbol::SymbolId, Type)> {
        let arena = self.ast_arena;
        let cache_key = (expr.index(), is_true_branch, self.current_scope);
        if let Some(cached) = self.narrowings_cache.get(&cache_key) {
            return cached.clone();
        }

        let mut narrowings = Vec::new();

        match &arena.expr(expr).kind {
            ExprKind::Unary {
                op: UnaryOp::Not,
                operand,
                ..
            } => {
                let operand = *operand;
                narrowings.extend(self.extract_narrowings(operand, bind, !is_true_branch));
            }

            ExprKind::Identifier { name } => {
                let scope = bind.scopes.get(self.current_scope);
                if let Some(id) = scope.resolve(*name, &bind.scopes) {
                    let original_ty = self
                        .symbol_types
                        .get(&id)
                        .cloned()
                        .or_else(|| bind.arena.get(id).ty.clone());
                    if let Some(ty) = original_ty {
                        if is_true_branch {
                            let narrowed = ty.non_nullified(&mut self.ty_table);
                            if !narrowed.is_dynamic() && narrowed != ty {
                                narrowings.push((id, narrowed));
                            }
                        } else if ty.is_nullable(&self.ty_table) {
                            narrowings.push((id, Type::Null));
                        }
                    }
                }
            }

            ExprKind::Binary { left, right, op } => {
                let (left, right, op) = (*left, *right, *op);
                let is_eq = op == BinaryOp::Eq;
                let is_neq = op == BinaryOp::NotEq;

                // 1. typeof x === "str" / "int" / "float" / "bool" / etc.
                let typeof_check = match (&arena.expr(left).kind, &arena.expr(right).kind) {
                    (
                        ExprKind::Unary {
                            op: UnaryOp::Typeof,
                            operand: typeof_op,
                            ..
                        },
                        ExprKind::StrLiteral { value },
                    ) => Some((*typeof_op, value.clone())),
                    (
                        ExprKind::StrLiteral { value },
                        ExprKind::Unary {
                            op: UnaryOp::Typeof,
                            operand: typeof_op,
                            ..
                        },
                    ) => Some((*typeof_op, value.clone())),
                    _ => None,
                };

                if let Some((typeof_op, value)) = typeof_check {
                    if (is_eq && is_true_branch) || (is_neq && !is_true_branch) {
                        if let ExprKind::Identifier { name } = &arena.expr(typeof_op).kind {
                            let scope = bind.scopes.get(self.current_scope);
                            if let Some(id) = scope.resolve(*name, &bind.scopes) {
                                let view = crate::binder::BindView::new(bind, self.resolver);
                                let narrowed_ty = crate::binder::resolve_primitive(
                                    &value,
                                    Some(&view),
                                    &mut self.ty_table,
                                );
                                narrowings.push((id, narrowed_ty));
                            }
                        }
                    }
                }

                // 2. x !== null / null !== x / x === null / null === x
                let (ident_name, is_null_check) =
                    match (&arena.expr(left).kind, &arena.expr(right).kind) {
                        (ExprKind::Identifier { name }, ExprKind::NullLiteral) => {
                            (Some(*name), true)
                        }
                        (ExprKind::NullLiteral, ExprKind::Identifier { name }) => {
                            (Some(*name), true)
                        }
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
                                    let narrowed = ty.non_nullified(&mut self.ty_table);
                                    if !narrowed.is_dynamic() {
                                        narrowings.push((id, narrowed));
                                    }
                                }
                            } else {
                                let name_str = bind.interner.resolve(name);
                                if name_str != "_" && name_str != "__variant__" {
                                    narrowings.push((id, Type::Null));
                                }
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
                    } = &arena.expr(left).kind
                    {
                        let (object, property) = (*object, *property);
                        if let (
                            ExprKind::Identifier { name: obj_name },
                            ExprKind::Identifier { name: prop_name },
                        ) = (&arena.expr(object).kind, &arena.expr(property).kind)
                        {
                            let (obj_name, prop_name) = (*obj_name, *prop_name);
                            let disc_ty: Option<Type> = match &arena.expr(right).kind {
                                ExprKind::StrLiteral { .. } => Some(Type::Str),
                                ExprKind::IntLiteral { .. } => Some(Type::Int),
                                _ => None,
                            };
                            if let Some(disc_ty) = disc_ty {
                                let prop_name_str = bind.interner.resolve(prop_name);
                                let scope = bind.scopes.get(self.current_scope);
                                if let Some(id) = scope.resolve(obj_name, &bind.scopes) {
                                    let original_ty = bind.arena.get(id).ty;
                                    let union_list = original_ty.and_then(|t| {
                                        match self.ty_table.get(t.0) {
                                            TypeKind::Union(list) => Some(list),
                                            _ => None,
                                        }
                                    });
                                    if let Some(list) = union_list {
                                        let members: Vec<Type> = self
                                            .ty_table
                                            .get_list(list)
                                            .iter()
                                            .map(|id| Type(*id, false))
                                            .collect();
                                        let mut matched: Vec<Type> = Vec::new();
                                        let mut unmatched: Vec<Type> = Vec::new();
                                        for m in members.iter() {
                                            let m_kind = self.ty_table.get(m.0);
                                            let hits = match m_kind {
                                                TypeKind::Object(mid) => {
                                                    self.ty_table.get_object_members(mid).iter().any(|f| match f {
                                                        ObjectTypeMember::Property {
                                                            name,
                                                            ty,
                                                            ..
                                                        } => {
                                                            name.as_ref() == prop_name_str
                                                                && *ty == disc_ty.0
                                                        }
                                                        _ => false,
                                                    })
                                                }
                                                TypeKind::Named(cn, _) => {
                                                    let cn_str = bind.interner.resolve(cn).to_string();
                                                    bind.get_interface_members_local(&cn_str)
                                                        .or_else(|| {
                                                            bind.get_class_entry(&cn_str)
                                                                .map(|e| &e.members)
                                                        })
                                                        .is_some_and(|ms| {
                                                            ms.iter().any(|cm| {
                                                                cm.name.as_ref() == prop_name_str
                                                                    && cm.ty == disc_ty
                                                            })
                                                        })
                                                }
                                                _ => false,
                                            };
                                            if hits {
                                                matched.push(*m);
                                            } else {
                                                unmatched.push(*m);
                                            }
                                        }
                                        let make_ty = |v: Vec<Type>, table: &mut crate::types::CheckerTyTable| match v.len() {
                                            0 => None,
                                            1 => Some(v.into_iter().next().unwrap()),
                                            _ => Some(Type::union(v, table)),
                                        };
                                        if (is_eq && is_true_branch) || (is_neq && !is_true_branch)
                                        {
                                            if !matched.is_empty() {
                                                if let Some(t) = make_ty(matched, &mut self.ty_table) {
                                                    narrowings.push((id, t));
                                                }
                                            }
                                        } else if ((is_neq && is_true_branch)
                                            || (is_eq && !is_true_branch))
                                            && !matched.is_empty()
                                        {
                                            if let Some(t) = make_ty(unmatched, &mut self.ty_table) {
                                                narrowings.push((id, t));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // 4. Instanceof narrowing: x instanceof User
                if op == BinaryOp::Instanceof {
                    if let (
                        ExprKind::Identifier { name },
                        ExprKind::Identifier { name: class_name },
                    ) = (&arena.expr(left).kind, &arena.expr(right).kind)
                    {
                        let (name, class_name) = (*name, *class_name);
                        let scope = bind.scopes.get(self.current_scope);
                        if let Some(id) = scope.resolve(name, &bind.scopes) {
                            let class_name_str = bind.interner.resolve(class_name).to_string();
                            if is_true_branch {
                                let named =
                                    Type::named(class_name_str, self.resolver, &mut self.ty_table);
                                narrowings.push((id, named));
                            } else if let Some(ty) = bind.arena.get(id).ty {
                                let class_name_atom = self.resolver.intern(&class_name_str);
                                let narrowed = ty.minus_named(class_name_atom, &mut self.ty_table);
                                if bind.interner.get("_") == Some(name) {
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
                let (left, right) = (*left, *right);
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
                let (left, right) = (*left, *right);
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
                let (expression, type_ann) = (*expression, type_ann.clone());
                if let ExprKind::Identifier { name: arg_name } = &arena.expr(expression).kind {
                    let arg_name = *arg_name;
                    let scope = bind.scopes.get(self.current_scope);
                    if let Some(id) = scope.resolve(arg_name, &bind.scopes) {
                        if is_true_branch {
                            let view = crate::binder::BindView::new(bind, self.resolver);
                            let narrowed_ty = crate::binder::resolve_type_node(
                                &type_ann,
                                Some(&view),
                                &mut self.ty_table,
                            );
                            narrowings.push((id, narrowed_ty));
                        } else {
                            if let Some(original_ty) = bind.arena.get(id).ty {
                                let view = crate::binder::BindView::new(bind, self.resolver);
                                let target_ty = crate::binder::resolve_type_node(
                                    &type_ann,
                                    Some(&view),
                                    &mut self.ty_table,
                                );
                                let narrowed = original_ty.minus(&target_ty, &mut self.ty_table);
                                if narrowed != original_ty {
                                    narrowings.push((id, narrowed));
                                }
                            }
                        }
                    }
                }
            }

            ExprKind::Call { callee, args, .. } => {
                let (callee, args) = (*callee, args.clone());
                let callee_ty_raw = self.infer_type(callee, bind);
                let callee_ty = callee_ty_raw.non_nullified(&mut self.ty_table);
                if let TypeKind::Fn(fid) = self.ty_table.get(callee_ty.0) {
                    let ft = self.ty_table.get_function(fid).clone();
                    if let TypeKind::TypePredicate {
                        parameter_name,
                        target_type,
                    } = self.ty_table.get(ft.return_type)
                    {
                        let target_type = Type(target_type, false);
                        let parameter_name_str = bind.interner.resolve(parameter_name);
                        let arg_expr = if let Some(pos) = ft
                            .params
                            .iter()
                            .position(|p| p.name.as_deref() == Some(parameter_name_str))
                        {
                            args.get(pos).and_then(|a| match a {
                                varn_core::ast::Arg::Positional(e) => Some(*e),
                                _ => None,
                            })
                        } else if args.len() == 1 {
                            match &args[0] {
                                varn_core::ast::Arg::Positional(e) => Some(*e),
                                _ => None,
                            }
                        } else {
                            None
                        };

                        if let Some(ExprKind::Identifier { name: arg_name }) =
                            arg_expr.map(|e| &arena.expr(e).kind)
                        {
                            let scope = bind.scopes.get(self.current_scope);
                            if let Some(id) = scope.resolve(*arg_name, &bind.scopes) {
                                let original_ty = self
                                    .symbol_types
                                    .get(&id)
                                    .cloned()
                                    .or_else(|| bind.arena.get(id).ty);
                                if is_true_branch {
                                    if let Some(orig) = original_ty {
                                        let orig_kind = self.ty_table.get(orig.0);
                                        let target_kind = self.ty_table.get(target_type.0);
                                        let matched: Vec<Type> = match orig_kind {
                                            TypeKind::Union(list) => self
                                                .ty_table
                                                .get_list(list)
                                                .to_vec()
                                                .into_iter()
                                                .map(|id| Type(id, false))
                                                .filter(|m| {
                                                    let m_kind = self.ty_table.get(m.0);
                                                    match (m_kind, target_kind) {
                                                        (
                                                            TypeKind::Array(_),
                                                            TypeKind::Array(_),
                                                        ) => true,
                                                        _ => *m == target_type,
                                                    }
                                                })
                                                .collect(),
                                            _ => vec![target_type],
                                        };
                                        if !matched.is_empty() {
                                            let narrowed = if matched.len() == 1 {
                                                matched.into_iter().next().unwrap()
                                            } else {
                                                Type::union(matched, &mut self.ty_table)
                                            };
                                            narrowings.push((id, narrowed));
                                        } else {
                                            narrowings.push((id, target_type));
                                        }
                                    } else {
                                        narrowings.push((id, target_type));
                                    }
                                } else if let Some(original_ty) = original_ty {
                                    let narrowed =
                                        original_ty.minus(&target_type, &mut self.ty_table);
                                    if narrowed != original_ty {
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
        subject: ExprId,
        bind: &BindResult,
    ) -> Option<(SymbolId, Vec<Type>)> {
        let arena = self.ast_arena;
        if let ExprKind::Member {
            object,
            property,
            computed: false,
            ..
        } = &arena.expr(subject).kind
        {
            let (object, property) = (*object, *property);
            if let (
                ExprKind::Identifier { name: obj_name },
                ExprKind::Identifier { name: _prop_name },
            ) = (&arena.expr(object).kind, &arena.expr(property).kind)
            {
                let obj_name = *obj_name;
                let scope = bind.scopes.get(self.current_scope);
                if let Some(id) = scope.resolve(obj_name, &bind.scopes) {
                    if let Some(ty) = bind.arena.get(id).ty {
                        if let TypeKind::Union(list) = self.ty_table.get(ty.0) {
                            let members: Vec<Type> = self
                                .ty_table
                                .get_list(list)
                                .iter()
                                .map(|id| Type(*id, false))
                                .collect();
                            return Some((id, members));
                        }
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
        subject: Option<ExprId>,
        bind: &BindResult,
    ) -> bool {
        let arena = self.ast_arena;
        let Some(disc_ty) = disc_ty else { return false };
        let Some(subject) = subject else { return false };
        let ExprKind::Member {
            property,
            computed: false,
            ..
        } = &arena.expr(subject).kind
        else {
            return false;
        };
        let property = *property;
        let ExprKind::Identifier { name: prop_name } = &arena.expr(property).kind else {
            return false;
        };
        let prop_name = bind.interner.resolve(*prop_name);

        match self.ty_table.get(m.0) {
            TypeKind::Object(mid) => self.ty_table.get_object_members(mid).iter().any(|f| match f {
                ObjectTypeMember::Property { name, ty, .. } => {
                    name.as_ref() == prop_name && *ty == disc_ty.0
                }
                _ => false,
            }),
            TypeKind::Named(cn, _) => {
                let cn_str = bind.interner.resolve(cn).to_string();
                bind.get_interface_members_local(&cn_str)
                    .or_else(|| bind.get_class_entry(&cn_str).map(|e| &e.members))
                    .is_some_and(|ms| {
                        ms.iter()
                            .any(|cm| cm.name.as_ref() == prop_name && cm.ty == *disc_ty)
                    })
            }
            _ => false,
        }
    }

    fn merge_narrowings(
        &mut self,
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
                    merged.push((id, types[0]));
                }
            } else if is_union {
                merged.push((id, Type::union(types, &mut self.ty_table)));
            } else {
                let ids: Vec<crate::types::CheckerTyId> = types.iter().map(|t| t.0).collect();
                let list = self.ty_table.intern_list(&ids);
                let interned = self.ty_table.intern(TypeKind::Intersection(list));
                merged.push((id, Type(interned, false)));
            }
        }
        merged
    }
}
