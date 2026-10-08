use super::Checker;
use crate::binder::widen_literal;
use crate::binder::BindResult;
use crate::types::Type;
use varn_core::ast::ExprId;

impl<'r> Checker<'r> {
    pub(super) fn infer_array(
        &mut self,
        elements: &[varn_core::ast::ArrayEl],
        bind: &BindResult,
    ) -> Type {
        let mut elem_tys = Vec::new();
        for el in elements {
            match el {
                varn_core::ast::ArrayEl::Expr(e) => {
                    let ty = self.infer_type(*e, bind);
                    if !ty.is_dynamic() {
                        elem_tys.push(ty);
                    }
                }
                varn_core::ast::ArrayEl::Spread(e) => {
                    let ty = self.infer_type(*e, bind);
                    if let varn_core::TypeKind::Array(inner) = self.ty_table.get(ty.0) {
                        elem_tys.push(Type::resolved(inner));
                    }
                }
                varn_core::ast::ArrayEl::Hole => {}
            }
        }
        if elem_tys.is_empty() {
            if let Some(expected) = self.expected_type {
                let non_null =
                    expected.non_nullified(&mut *std::sync::Arc::make_mut(&mut self.ty_table));
                if let varn_core::TypeKind::Array(inner) = self.ty_table.get(non_null.0) {
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
                let widened = widen_literal(first);
                Type::array(widened, &mut *std::sync::Arc::make_mut(&mut self.ty_table))
            } else {
                let unioned =
                    Type::union(elem_tys, &mut *std::sync::Arc::make_mut(&mut self.ty_table));
                let widened = widen_literal(unioned);
                Type::array(widened, &mut *std::sync::Arc::make_mut(&mut self.ty_table))
            }
        }
    }

    pub(super) fn infer_tuple(&mut self, elements: &[ExprId], bind: &BindResult) -> Type {
        let elem_tys: Vec<Type> = elements.iter().map(|e| self.infer_type(*e, bind)).collect();
        let ids: Vec<crate::types::CheckerTyId> = elem_tys.iter().map(|t| t.0).collect();
        let list = std::sync::Arc::make_mut(&mut self.ty_table).intern_list(&ids);
        Type::resolved(
            std::sync::Arc::make_mut(&mut self.ty_table).intern(varn_core::TypeKind::Tuple(list)),
        )
    }

    pub(super) fn infer_record(
        &mut self,
        properties: &[varn_core::ast::ObjectProp],
        bind: &BindResult,
    ) -> Type {
        let mut members = Vec::new();
        for prop in properties {
            if let varn_core::ast::ObjectProp::Property { key, value, .. } = prop {
                let ty = self.infer_type(*value, bind);
                let name: std::sync::Arc<str> = match &key {
                    varn_core::ast::PropKey::Identifier(s) | varn_core::ast::PropKey::Str(s) => {
                        std::sync::Arc::from(s.as_str())
                    }
                    varn_core::ast::PropKey::Int(n) => std::sync::Arc::from(n.to_string().as_str()),
                    varn_core::ast::PropKey::Computed(_) => std::sync::Arc::from("<computed>"),
                };
                members.push(crate::types::ObjectTypeMember::Property {
                    name,
                    ty: ty.0,
                    optional: false,
                    readonly: true,
                });
            }
        }
        Type::object(members, &mut *std::sync::Arc::make_mut(&mut self.ty_table))
    }

    pub(super) fn infer_match(
        &mut self,
        subject: varn_core::ast::ExprId,
        cases: &[varn_core::ast::MatchCase],
        arena: &varn_core::ast::AstArena,
        bind: &BindResult,
    ) -> Type {
        let arm_scopes = bind.match_arm_scopes.get(&subject.index());
        let mut tys = Vec::new();
        for (i, case) in cases.iter().enumerate() {
            match &case.body {
                varn_core::ast::MatchBody::Expr(e) => {
                    let saved_scope = self.current_scope;
                    if let Some(&scope) = arm_scopes.and_then(|s| s.get(i)) {
                        self.current_scope = scope;
                    }
                    let ty = self.infer_type(*e, bind);
                    self.current_scope = saved_scope;
                    tys.push(ty);
                }
                varn_core::ast::MatchBody::Block(stmt) => {
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
