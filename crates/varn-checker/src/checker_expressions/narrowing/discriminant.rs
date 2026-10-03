use crate::binder::BindResult;
use crate::checker::Checker;
use crate::symbol::SymbolId;
use crate::types::{ObjectTypeMember, Type};
use varn_core::ast::operators::BinaryOp;
use varn_core::ast::{ExprId, ExprKind};
use varn_core::TypeKind;

impl<'r> Checker<'r> {
    pub(crate) fn narrow_discriminant(
        &mut self,
        left: ExprId,
        right: ExprId,
        op: BinaryOp,
        bind: &BindResult,
        is_true_branch: bool,
        out: &mut Vec<(SymbolId, Type)>,
    ) {
        let arena = self.ast_arena;
        let is_eq = op == BinaryOp::Eq;
        let is_neq = op == BinaryOp::NotEq;
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
                            let union_list =
                                original_ty.and_then(|t| match self.ty_table.get(t.0) {
                                    TypeKind::Union(list) => Some(list),
                                    _ => None,
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
                                        TypeKind::Object(mid) => self
                                            .ty_table
                                            .get_object_members(mid)
                                            .iter()
                                            .any(|f| match f {
                                                ObjectTypeMember::Property {
                                                    name,
                                                    ty,
                                                    ..
                                                } => {
                                                    name.as_ref() == prop_name_str
                                                        && *ty == disc_ty.0
                                                }
                                                _ => false,
                                            }),
                                        TypeKind::Named(cn, _) => {
                                            let cn_str =
                                                bind.interner.resolve(cn).to_string();
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
                                        if let Some(t) = make_ty(
                                            matched,
                                            &mut *std::sync::Arc::make_mut(
                                                &mut self.ty_table,
                                            ),
                                        ) {
                                            out.push((id, t));
                                        }
                                    }
                                } else if ((is_neq && is_true_branch)
                                    || (is_eq && !is_true_branch))
                                    && !matched.is_empty()
                                {
                                    if let Some(t) = make_ty(
                                        unmatched,
                                        &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                                    ) {
                                        out.push((id, t));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
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
            TypeKind::Object(mid) => {
                self.ty_table
                    .get_object_members(mid)
                    .iter()
                    .any(|f| match f {
                        ObjectTypeMember::Property { name, ty, .. } => {
                            name.as_ref() == prop_name && *ty == disc_ty.0
                        }
                        _ => false,
                    })
            }
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
}
