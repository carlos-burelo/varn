use super::Checker;
use crate::checker::recorder::Recorder;
use varn_core::ast::ExprId;
use varn_sem::bind::BindResult;
use varn_sem::types::Type;

impl<'r> Checker<'r> {
    fn field_value_type(&mut self, target_ty: Type, bind: &BindResult) -> Type {
        if target_ty.is_unresolved() {
            return target_ty;
        }
        let members = crate::checker_expressions::members::get_members_of_type(
            self.resolver,
            &target_ty,
            bind,
            std::sync::Arc::make_mut(&mut self.ty_table),
        );
        let field_tys: Vec<Type> = members
            .iter()
            .filter(|m| {
                matches!(m.kind, varn_sem::semantic_info::ResolvedMemberKind::Property)
                    && !m.is_static
            })
            .map(|m| m.ty)
            .collect();
        if field_tys.is_empty() {
            return Type::Never;
        }
        Type::union(
            field_tys,
            &mut *std::sync::Arc::make_mut(&mut self.ty_table),
        )
    }

    pub(super) fn infer_meta_access(
        &mut self,
        rec: &mut Recorder,
        target: ExprId,
        property: varn_core::Atom,
        bind: &BindResult,
    ) -> Type {
        let target_ty = self.infer_type(rec, target, bind);
        match varn_core::MemberKey::from_str(bind.interner.resolve(property)) {
            Some(varn_core::MemberKey::Name) | Some(varn_core::MemberKey::Type) => Type::Str,
            Some(varn_core::MemberKey::Class) => Type::builtin(
                varn_core::BuiltinType::Class,
                vec![target_ty],
                &mut *std::sync::Arc::make_mut(&mut self.ty_table),
            ),
            Some(varn_core::MemberKey::Fields) | Some(varn_core::MemberKey::Methods) => {
                Type::array(
                    Type::Str,
                    &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                )
            }
            Some(varn_core::MemberKey::Keys) => {
                let ret = Type::array(
                    Type::Str,
                    &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                );
                Type::fn_(
                    varn_sem::types::FunctionType {
                        params: vec![],
                        return_type: ret.0,
                        is_arrow: true,
                        type_params: vec![],
                        type_param_defaults: Vec::new(),
                    },
                    &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                )
            }
            Some(varn_core::MemberKey::Values) => {
                let value_ty = self.field_value_type(target_ty, bind);
                let ret = Type::array(
                    value_ty,
                    &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                );
                Type::fn_(
                    varn_sem::types::FunctionType {
                        params: vec![],
                        return_type: ret.0,
                        is_arrow: true,
                        type_params: vec![],
                        type_param_defaults: Vec::new(),
                    },
                    &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                )
            }
            Some(varn_core::MemberKey::Entries) => {
                let value_ty = self.field_value_type(target_ty, bind);
                let ids: Vec<varn_sem::types::CheckerTyId> = vec![Type::Str.0, value_ty.0];
                let list = std::sync::Arc::make_mut(&mut self.ty_table).intern_list(&ids);
                let entry = Type::resolved(
                    std::sync::Arc::make_mut(&mut self.ty_table)
                        .intern(varn_core::TypeKind::Tuple(list)),
                );
                let ret = Type::array(entry, &mut *std::sync::Arc::make_mut(&mut self.ty_table));
                Type::fn_(
                    varn_sem::types::FunctionType {
                        params: vec![],
                        return_type: ret.0,
                        is_arrow: true,
                        type_params: vec![],
                        type_param_defaults: Vec::new(),
                    },
                    &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                )
            }
            Some(varn_core::MemberKey::HasOwn) => Type::fn_(
                varn_sem::types::FunctionType {
                    params: vec![varn_sem::types::FunctionParam {
                        name: Some(std::sync::Arc::from("key")),
                        ty: Type::Str.0,
                        optional: false,
                        is_rest: false,
                    }],
                    return_type: Type::Bool.0,
                    is_arrow: true,
                    type_params: vec![],
                    type_param_defaults: Vec::new(),
                },
                &mut *std::sync::Arc::make_mut(&mut self.ty_table),
            ),
            None | Some(_) => Type::Dynamic,
        }
    }
}
