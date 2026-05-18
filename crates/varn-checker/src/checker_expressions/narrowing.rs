use crate::binder::BindResult;
use crate::checker::Checker;
use crate::symbol::SymbolId;
use crate::types::{ObjectTypeMember, Type};
use rustc_hash::FxHashMap;
use varn_core::ast::operators::BinaryOp;
use varn_core::ast::{Expr, ExprKind};
use varn_core::TypeKind;

impl Checker {
    pub(crate) fn can_extract_narrowings(&self, expr: &Expr) -> bool {
        matches!(
            &expr.kind,
            ExprKind::Binary {
                op: BinaryOp::Eq | BinaryOp::NotEq | BinaryOp::Instanceof,
                ..
            } | ExprKind::Logical { .. }
                | ExprKind::Is { .. }
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
            ExprKind::Binary { left, right, op } => {
                let is_eq = *op == varn_core::ast::operators::BinaryOp::Eq;
                let is_neq = *op == varn_core::ast::operators::BinaryOp::NotEq;

                if let (
                    ExprKind::Unary {
                        op: varn_core::ast::operators::UnaryOp::Typeof,
                        operand: typeof_op,
                        ..
                    },
                    ExprKind::StrLiteral { value },
                ) = (&left.kind, &right.kind)
                {
                    if (is_eq && is_true_branch) || (is_neq && !is_true_branch) {
                        if let ExprKind::Identifier { name } = &typeof_op.kind {
                            let scope = bind.scopes.get(self.current_scope);
                            if let Some(id) = scope.resolve(name, &bind.scopes) {
                                let narrowed_ty =
                                    crate::binder::resolve_primitive(value, Some(bind));
                                narrowings.push((id, narrowed_ty));
                            }
                        }
                    }
                }

                if let (ExprKind::Identifier { name }, ExprKind::NullLiteral) =
                    (&left.kind, &right.kind)
                {
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
                                ExprKind::StrLiteral { value } => {
                                    Some(Type::literal_str(value.as_ref()))
                                }
                                ExprKind::IntLiteral { value, .. } => {
                                    Some(Type::literal_int(*value))
                                }
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
                                crate::binder::resolve_type_node(type_ann, Some(bind));
                            narrowings.push((id, narrowed_ty));
                        } else {
                            if let Some(original_ty) = &bind.arena.get(id).ty {
                                let target_ty =
                                    crate::binder::resolve_type_node(type_ann, Some(bind));
                                let narrowed = original_ty.minus(&target_ty);
                                if narrowed != *original_ty {
                                    narrowings.push((id, narrowed));
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
            computed: false,
            ..
        } = &subject.kind
        {
            if let ExprKind::Identifier { name: obj_name } = &object.kind {
                let scope = bind.scopes.get(self.current_scope);
                let id = scope.resolve(obj_name, &bind.scopes)?;
                let sym = bind.arena.get(id);
                if let Some(Type(TypeKind::Union(members), _)) = &sym.ty {
                    return Some((id, members.clone()));
                }
            }
        }
        None
    }

    pub(crate) fn union_member_matches_disc(
        &self,
        member: &Type,
        disc_ty: Option<&Type>,
        subject: Option<&Expr>,
        bind: &BindResult,
    ) -> bool {
        let disc_ty = match disc_ty {
            Some(t) => t,
            None => return false,
        };
        let prop_name = match subject {
            Some(s) => match &s.kind {
                ExprKind::Member {
                    property,
                    computed: false,
                    ..
                } => {
                    if let ExprKind::Identifier { name } = &property.kind {
                        name.as_ref()
                    } else {
                        return false;
                    }
                }
                _ => return false,
            },
            None => return false,
        };
        match &member.0 {
            TypeKind::Object(fields) => fields.iter().any(|f| match f {
                ObjectTypeMember::Property { name, ty, .. } => {
                    name.as_ref() == prop_name && ty == disc_ty
                }
                _ => false,
            }),
            TypeKind::Named(cn, _) => bind
                .get_interface_members_local(cn.as_ref())
                .or_else(|| bind.get_class_entry(cn.as_ref()).map(|e| &e.members))
                .is_some_and(|ms| {
                    ms.iter()
                        .any(|cm| cm.name.as_ref() == prop_name && cm.ty == *disc_ty)
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
        let mut result = Vec::new();
        let mut right_map: FxHashMap<SymbolId, Type> = right.into_iter().collect();

        for (id, ty_l) in left {
            if let Some(ty_r) = right_map.remove(&id) {
                let merged = if is_union {
                    Type::union(vec![ty_l, ty_r])
                } else {
                    ty_l
                };
                result.push((id, merged));
            } else if !is_union {
                result.push((id, ty_l));
            }
        }

        if !is_union {
            for (id, ty_r) in right_map {
                result.push((id, ty_r));
            }
        }

        result
    }
}
