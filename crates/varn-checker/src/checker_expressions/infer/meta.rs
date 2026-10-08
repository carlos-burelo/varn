use super::Checker;
use crate::binder::BindResult;
use crate::types::Type;
use varn_core::ast::ExprId;

impl<'r> Checker<'r> {
    pub(super) fn infer_meta_access(
        &mut self,
        target: ExprId,
        property: varn_core::Atom,
        bind: &BindResult,
    ) -> Type {
        let _target_ty = self.infer_type(target, bind);
        match varn_core::MemberKey::from_str(bind.interner.resolve(property)) {
            Some(varn_core::MemberKey::Name) | Some(varn_core::MemberKey::Type) => Type::Str,
            Some(varn_core::MemberKey::Class) => Type::Dynamic,
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
                    crate::types::FunctionType {
                        params: vec![],
                        return_type: ret.0,
                        is_arrow: true,
                        type_params: vec![],
                    },
                    &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                )
            }
            Some(varn_core::MemberKey::Values) => {
                let ret = Type::array(
                    Type::Dynamic,
                    &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                );
                Type::fn_(
                    crate::types::FunctionType {
                        params: vec![],
                        return_type: ret.0,
                        is_arrow: true,
                        type_params: vec![],
                    },
                    &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                )
            }
            Some(varn_core::MemberKey::Entries) => {
                let ids: Vec<crate::types::CheckerTyId> = vec![Type::Str.0, Type::Dynamic.0];
                let list = std::sync::Arc::make_mut(&mut self.ty_table).intern_list(&ids);
                let entry = Type::resolved(
                    std::sync::Arc::make_mut(&mut self.ty_table)
                        .intern(varn_core::TypeKind::Tuple(list)),
                );
                let ret = Type::array(entry, &mut *std::sync::Arc::make_mut(&mut self.ty_table));
                Type::fn_(
                    crate::types::FunctionType {
                        params: vec![],
                        return_type: ret.0,
                        is_arrow: true,
                        type_params: vec![],
                    },
                    &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                )
            }
            Some(varn_core::MemberKey::HasOwn) => Type::fn_(
                crate::types::FunctionType {
                    params: vec![crate::types::FunctionParam {
                        name: Some(std::sync::Arc::from("key")),
                        ty: Type::Str.0,
                        optional: false,
                        is_rest: false,
                    }],
                    return_type: Type::Bool.0,
                    is_arrow: true,
                    type_params: vec![],
                },
                &mut *std::sync::Arc::make_mut(&mut self.ty_table),
            ),
            None | Some(_) => Type::Dynamic,
        }
    }
}
