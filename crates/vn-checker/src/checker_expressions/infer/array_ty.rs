use super::Checker;
use crate::checker::recorder::Recorder;
use vn_core::ast::ExprId;
use vn_sem::bind::BindResult;
use vn_sem::types::Type;

impl<'r> Checker<'r> {
    pub(super) fn infer_array(
        &mut self,
        rec: &mut Recorder,
        expr: vn_core::ast::ExprId,
        elements: &[vn_core::ast::ArrayEl],
        bind: &BindResult,
    ) -> Type {
        let mut elem_tys = Vec::new();
        let mut holds_dynamic = false;
        for el in elements {
            match el {
                vn_core::ast::ArrayEl::Expr(e) => {
                    let ty = self.infer_type(rec, *e, bind);
                    if ty.is_unresolved() {
                        holds_dynamic = true;
                    } else {
                        elem_tys.push(ty);
                    }
                }
                vn_core::ast::ArrayEl::Spread(e) => {
                    let ty = self.infer_type(rec, *e, bind);
                    let spread: Vec<Type> = match self.ty_table.get(ty.0) {
                        vn_core::TypeKind::Array(inner) => vec![Type::resolved(inner)],
                        vn_core::TypeKind::Tuple(items) => self
                            .ty_table
                            .get_list(items)
                            .iter()
                            .map(|item| Type::resolved(*item))
                            .collect(),
                        vn_core::TypeKind::Primitive(_)
                        | vn_core::TypeKind::Literal(_)
                        | vn_core::TypeKind::This
                        | vn_core::TypeKind::Union(_)
                        | vn_core::TypeKind::Intersection(_)
                        | vn_core::TypeKind::Named(..)
                        | vn_core::TypeKind::Generic(..)
                        | vn_core::TypeKind::TemplateLiteral(_)
                        | vn_core::TypeKind::Fn(_)
                        | vn_core::TypeKind::Record(_)
                        | vn_core::TypeKind::Interface(_)
                        | vn_core::TypeKind::Typeof(_)
                        | vn_core::TypeKind::KeyOf(_)
                        | vn_core::TypeKind::IndexedAccess { .. }
                        | vn_core::TypeKind::Mapped { .. }
                        | vn_core::TypeKind::Conditional { .. }
                        | vn_core::TypeKind::Infer(_)
                        | vn_core::TypeKind::EnumVariant { .. }
                        | vn_core::TypeKind::TypePredicate { .. } => vec![Type::Dynamic],
                    };
                    for item in spread {
                        if item.is_unresolved() {
                            holds_dynamic = true;
                        } else {
                            elem_tys.push(item);
                        }
                    }
                }
                vn_core::ast::ArrayEl::Hole => {}
            }
        }
        if holds_dynamic && !elem_tys.is_empty() {
            return Type::array(
                Type::Dynamic,
                &mut *std::sync::Arc::make_mut(&mut self.ty_table),
            );
        }
        if elem_tys.is_empty() {
            if let Some(expected) = self.expected_for(expr) {
                let non_null =
                    expected.non_nullified(&mut *std::sync::Arc::make_mut(&mut self.ty_table));
                if let vn_core::TypeKind::Array(inner) = self.ty_table.get(non_null.0) {
                    Type::array(
                        Type::resolved(inner),
                        &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                    )
                } else {
                    Type::array(
                        Type::Dynamic,
                        &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                    )
                }
            } else {
                Type::array(
                    Type::Dynamic,
                    &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                )
            }
        } else {
            let first = elem_tys[0];
            if elem_tys.iter().all(|t| t == &first) {
                Type::array(first, &mut *std::sync::Arc::make_mut(&mut self.ty_table))
            } else {
                let unioned =
                    Type::union(elem_tys, &mut *std::sync::Arc::make_mut(&mut self.ty_table));
                Type::array(unioned, &mut *std::sync::Arc::make_mut(&mut self.ty_table))
            }
        }
    }

    pub(super) fn infer_tuple(
        &mut self,
        rec: &mut Recorder,
        elements: &[ExprId],
        bind: &BindResult,
    ) -> Type {
        let elem_tys: Vec<Type> = elements
            .iter()
            .map(|e| self.infer_type(rec, *e, bind))
            .collect();
        let ids: Vec<vn_sem::types::CheckerTyId> = elem_tys.iter().map(|t| t.0).collect();
        let list = std::sync::Arc::make_mut(&mut self.ty_table).intern_list(&ids);
        Type::resolved(
            std::sync::Arc::make_mut(&mut self.ty_table).intern(vn_core::TypeKind::Tuple(list)),
        )
    }

    pub(super) fn infer_match(
        &mut self,
        rec: &mut Recorder,
        subject: vn_core::ast::ExprId,
        cases: &[vn_core::ast::MatchCase],
        arena: &vn_core::ast::AstArena,
        bind: &BindResult,
    ) -> Type {
        let arm_scopes = bind.match_arm_scopes.get(&subject.index());
        let mut tys = Vec::new();
        for (i, case) in cases.iter().enumerate() {
            match &case.body {
                vn_core::ast::MatchBody::Expr(e) => {
                    let saved_scope = self.current_scope;
                    if let Some(&scope) = arm_scopes.and_then(|s| s.get(i)) {
                        self.current_scope = scope;
                    }
                    let ty = self.infer_type(rec, *e, bind);
                    self.current_scope = saved_scope;
                    tys.push(ty);
                }
                vn_core::ast::MatchBody::Block(stmt) => {
                    if !crate::checker::completion::can_complete_normally(*stmt, arena) {
                        tys.push(Type::Never);
                    } else {
                        tys.push(Type::Void);
                    }
                }
            }
        }
        let non_never: Vec<Type> = tys.iter().filter(|t| !t.is_never()).cloned().collect();
        if non_never.is_empty() {
            if tys.is_empty() {
                Type::Dynamic
            } else {
                Type::Never
            }
        } else if non_never.len() == 1 {
            non_never.into_iter().next().unwrap()
        } else {
            Type::union(
                non_never,
                &mut *std::sync::Arc::make_mut(&mut self.ty_table),
            )
        }
    }
}
