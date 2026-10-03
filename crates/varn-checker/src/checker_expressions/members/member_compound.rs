use super::member_util::intrinsic_member_info;
use super::Checker;
use crate::binder::BindResult;
use crate::types::{ObjectTypeMember, Type};
use std::sync::Arc;
use varn_core::TypeKind;

impl<'r> Checker<'r> {
    pub(super) fn member_generic(
        &mut self,
        name_atom: varn_core::Atom,
        args_list: crate::types::TyListId,
        origin_atom: Option<varn_core::Atom>,
        key: &str,
        bind: &BindResult,
    ) -> Option<(Type, Option<usize>)> {
        let base = Type::named_with_origin_atom(
            name_atom,
            origin_atom,
            &mut *std::sync::Arc::make_mut(&mut self.ty_table),
        );
        let name: Arc<str> = self.resolve_bind_atom(bind, name_atom);
        let origin: Option<Arc<str>> = origin_atom.map(|o| self.resolve_bind_atom(bind, o));
        let args: Vec<Type> = self
            .ty_table
            .get_list(args_list)
            .iter()
            .map(|id| Type::resolved(*id))
            .collect();
        self.find_member_info_uncached(&base, key, bind)
            .map(|(member_ty, sym)| {
                let mapping = super::member_util::generic_mapping(
                    self.resolver,
                    name.as_ref(),
                    &args,
                    origin.as_ref(),
                    bind,
                );
                if mapping.is_empty() {
                    (member_ty, sym)
                } else {
                    (
                        member_ty.map_generics(
                            &mapping,
                            &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                        ),
                        sym,
                    )
                }
            })
    }

    pub(super) fn member_object(
        &mut self,
        mid: crate::types::ObjectMembersId,
        key: &str,
    ) -> Option<(Type, Option<usize>)> {
        let members = self.ty_table.get_object_members(mid).to_vec();
        members.iter().find(|m| m.name() == key).map(|m| {
            let ty = match m {
                ObjectTypeMember::Property {
                    ty, optional: true, ..
                } => Type::make_nullable(
                    Type::resolved(*ty),
                    &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                ),
                ObjectTypeMember::Property { ty, .. } => Type::resolved(*ty),
                ObjectTypeMember::Method {
                    params,
                    return_type,
                    is_arrow,
                    optional: true,
                    ..
                } => Type::make_nullable(
                    Type::fn_(
                        crate::types::FunctionType {
                            params: params.clone(),
                            return_type: *return_type,
                            is_arrow: *is_arrow,
                            type_params: vec![],
                        },
                        &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                    ),
                    &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                ),
                ObjectTypeMember::Method {
                    params,
                    return_type,
                    is_arrow,
                    ..
                } => Type::fn_(
                    crate::types::FunctionType {
                        params: params.clone(),
                        return_type: *return_type,
                        is_arrow: *is_arrow,
                        type_params: vec![],
                    },
                    &mut *std::sync::Arc::make_mut(&mut self.ty_table),
                ),
                _ => Type::Dynamic,
            };
            (ty, None)
        })
    }

    pub(super) fn member_union(
        &mut self,
        list: crate::types::TyListId,
        key: &str,
        bind: &BindResult,
    ) -> Option<(Type, Option<usize>)> {
        let ids = self.ty_table.get_list(list).to_vec();
        let infos: Vec<(Type, Option<usize>)> = ids
            .iter()
            .filter_map(|id| self.find_member_info_uncached(&Type::resolved(*id), key, bind))
            .collect();
        if infos.len() == ids.len() {
            let first_sid = infos[0].1;
            let all_same_sid = infos.iter().all(|i| i.1 == first_sid);
            let types: Vec<Type> = infos.into_iter().map(|i| i.0).collect();

            let collapsed = if types.windows(2).all(|w| w[0] == w[1]) {
                types[0]
            } else {
                Type::union(types, &mut *std::sync::Arc::make_mut(&mut self.ty_table))
            };
            Some((collapsed, if all_same_sid { first_sid } else { None }))
        } else {
            None
        }
    }

    pub(super) fn member_array(
        &mut self,
        inner: crate::types::CheckerTyId,
        key: &str,
        bind: &BindResult,
    ) -> Option<(Type, Option<usize>)> {
        let table = std::sync::Arc::make_mut(&mut self.ty_table);
        let atom = table.intern_name(varn_core::BuiltinType::Array.name());
        let array_ty = Type::generic_atom(atom, vec![Type::resolved(inner)], None, table);
        self.find_member_info_uncached(&array_ty, key, bind)
    }

    pub(super) fn member_scalar(
        &self,
        kind: &crate::types::InternedTypeKind,
        key: &str,
        bind: &BindResult,
    ) -> Option<(Type, Option<usize>)> {
        match kind {
            TypeKind::Primitive(varn_core::LangPrimitive::Str) => {
                if key == varn_core::MemberKey::Length.as_str() {
                    Some((Type::Int, None))
                } else {
                    intrinsic_member_info(bind, varn_core::LangPrimitive::Str.name(), key)
                }
            }
            TypeKind::Builtin(varn_core::BuiltinType::Bytes) => {
                if key == varn_core::MemberKey::Length.as_str() {
                    Some((Type::Int, None))
                } else {
                    intrinsic_member_info(bind, varn_core::BuiltinType::Bytes.name(), key)
                }
            }
            TypeKind::Tuple(_) if key == varn_core::MemberKey::Length.as_str() => {
                Some((Type::Int, None))
            }
            kind @ (TypeKind::Primitive(_) | TypeKind::Builtin(_) | TypeKind::Literal(_)) => {
                let name = kind.lang_name().unwrap_or_default();
                intrinsic_member_info(bind, name, key)
            }
            _ => None,
        }
    }

    pub(super) fn member_range_fallback(
        &mut self,
        key: &str,
        bind: &BindResult,
    ) -> Option<(Type, Option<usize>)> {
        let table = std::sync::Arc::make_mut(&mut self.ty_table);
        let atom = table.intern_name(varn_core::BuiltinType::Range.name());
        let range_ty = Type::named_atom(atom, table);
        self.find_member_info_uncached(&range_ty, key, bind)
    }
}
