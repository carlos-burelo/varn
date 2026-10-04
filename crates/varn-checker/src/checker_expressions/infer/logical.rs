use super::Checker;
use crate::binder::BindResult;
use crate::types::Type;
use varn_core::ast::ExprId;
use varn_core::TypeKind;

impl<'r> Checker<'r> {
    pub(super) fn infer_logical(
        &mut self,
        op: varn_core::ast::LogicalOp,
        left: ExprId,
        right: ExprId,
        bind: &BindResult,
    ) -> Type {
        let l_ty = self.infer_type(left, bind);
        let r_ty = self.infer_type(right, bind);
        match op {
            varn_core::ast::LogicalOp::And => {
                if l_ty == r_ty {
                    l_ty
                } else {
                    Type::union(
                        vec![l_ty, r_ty],
                        &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                    )
                }
            }
            varn_core::ast::LogicalOp::Nullish => {
                let l_non_null =
                    l_ty.non_nullified(&mut *std::sync::Arc::make_mut(&mut self.ty_table));
                if l_non_null == r_ty {
                    r_ty
                } else {
                    Type::union(
                        vec![l_non_null, r_ty],
                        &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                    )
                }
            }
            varn_core::ast::LogicalOp::Or => {
                if l_ty == r_ty {
                    l_ty
                } else {
                    Type::union(
                        vec![l_ty, r_ty],
                        &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                    )
                }
            }
        }
    }

    pub(super) fn infer_try(&mut self, expression: ExprId, bind: &BindResult) -> Type {
        let ty = self.infer_type(expression, bind);
        match ty.core_sum(&self.ty_table, |a| bind.interner.try_resolve(a)) {
            Some((_, args)) => args.first().copied().unwrap_or(Type::Dynamic),
            None if ty.is_nullable(&self.ty_table) => {
                ty.non_nullified(&mut *std::sync::Arc::make_mut(&mut self.ty_table))
            }
            _ => Type::Dynamic,
        }
    }

    pub(super) fn infer_non_null(&mut self, expression: ExprId, bind: &BindResult) -> Type {
        let ty = self.infer_type(expression, bind);
        if let TypeKind::Union(list) = self.ty_table.get(ty.0) {
            let ids = self.ty_table.get_list(list).to_vec();
            let filtered: Vec<Type> = ids
                .into_iter()
                .filter(|id| {
                    !matches!(
                        self.ty_table.get(*id),
                        TypeKind::Primitive(varn_core::LangPrimitive::Null)
                            | TypeKind::Primitive(varn_core::LangPrimitive::Void)
                    )
                })
                .map(Type::resolved)
                .collect();
            if filtered.len() == 1 {
                return filtered[0];
            }
            return Type::union(filtered, &mut *std::sync::Arc::make_mut(&mut self.ty_table));
        }
        ty
    }
}
