use super::*;

impl Type {
    pub fn is_bytes(&self, table: &CheckerTyTable) -> bool {
        self.is_builtin(varn_core::BuiltinType::Bytes, table)
    }

    pub fn is_nullable(&self, table: &CheckerTyTable) -> bool {
        match table.get(self.0) {
            TypeKind::Primitive(varn_core::LangPrimitive::Null) => true,
            TypeKind::Union(list) => table
                .get_list(list)
                .iter()
                .any(|m| Type::resolved(*m).is_nullable(table)),
            TypeKind::Primitive(_)
            | TypeKind::Literal(_)
            | TypeKind::This
            | TypeKind::Array(_)
            | TypeKind::Intersection(_)
            | TypeKind::Tuple(_)
            | TypeKind::Named(..)
            | TypeKind::Generic(..)
            | TypeKind::TemplateLiteral(_)
            | TypeKind::Fn(_)
            | TypeKind::Record(_)
            | TypeKind::Interface(_)
            | TypeKind::Typeof(_)
            | TypeKind::KeyOf(_)
            | TypeKind::IndexedAccess { .. }
            | TypeKind::Mapped { .. }
            | TypeKind::Conditional { .. }
            | TypeKind::Infer(_)
            | TypeKind::EnumVariant { .. }
            | TypeKind::TypePredicate { .. } => false,
        }
    }

    pub fn make_nullable(ty: Type, table: &mut CheckerTyTable) -> Type {
        if ty.is_nullable(table) {
            ty
        } else {
            Type::union(vec![ty, Type::Null], table)
        }
    }

    pub fn non_nullified(&self, table: &mut CheckerTyTable) -> Type {
        match table.get(self.0) {
            TypeKind::Primitive(varn_core::LangPrimitive::Null) => Type::Never,
            TypeKind::Union(list) => {
                let members: Vec<Type> = table
                    .get_list(list)
                    .iter()
                    .map(|id| Type::resolved(*id))
                    .collect();
                let new_members: Vec<Type> = members
                    .into_iter()
                    .filter(|m| !m.is_nullable(table))
                    .collect();

                if new_members.is_empty() {
                    Type::Never
                } else if new_members.len() == 1 {
                    new_members.into_iter().next().unwrap()
                } else {
                    let ids: Vec<CheckerTyId> = new_members.iter().map(|m| m.0).collect();
                    let new_list = table.intern_list(&ids);
                    Type::resolved(table.intern(TypeKind::Union(new_list)))
                }
            }
            TypeKind::Primitive(_)
            | TypeKind::Literal(_)
            | TypeKind::This
            | TypeKind::Array(_)
            | TypeKind::Intersection(_)
            | TypeKind::Tuple(_)
            | TypeKind::Named(..)
            | TypeKind::Generic(..)
            | TypeKind::TemplateLiteral(_)
            | TypeKind::Fn(_)
            | TypeKind::Record(_)
            | TypeKind::Interface(_)
            | TypeKind::Typeof(_)
            | TypeKind::KeyOf(_)
            | TypeKind::IndexedAccess { .. }
            | TypeKind::Mapped { .. }
            | TypeKind::Conditional { .. }
            | TypeKind::Infer(_)
            | TypeKind::EnumVariant { .. }
            | TypeKind::TypePredicate { .. } => *self,
        }
    }

    pub fn minus_nominal(
        &self,
        name: varn_core::Atom,
        origin: Option<varn_core::Atom>,
        table: &mut CheckerTyTable,
    ) -> Type {
        let is_class = |kind: InternedTypeKind| matches!(kind, TypeKind::Named(n, o) | TypeKind::Generic(n, _, o) if n == name && o == origin);
        match table.get(self.0) {
            kind if is_class(kind) => Type::Never,
            TypeKind::Union(list) => {
                let members: Vec<CheckerTyId> = table.get_list(list).to_vec();
                let mut kept: Vec<CheckerTyId> = Vec::with_capacity(members.len());
                for id in members {
                    if !is_class(table.get(id)) {
                        kept.push(id);
                    }
                }
                if kept.is_empty() {
                    Type::Never
                } else if kept.len() == 1 {
                    Type::resolved(kept[0])
                } else {
                    let new_list = table.intern_list(&kept);
                    Type::resolved(table.intern(TypeKind::Union(new_list)))
                }
            }
            TypeKind::Primitive(_)
            | TypeKind::Literal(_)
            | TypeKind::This
            | TypeKind::Array(_)
            | TypeKind::Intersection(_)
            | TypeKind::Tuple(_)
            | TypeKind::Named(..)
            | TypeKind::Generic(..)
            | TypeKind::TemplateLiteral(_)
            | TypeKind::Fn(_)
            | TypeKind::Record(_)
            | TypeKind::Interface(_)
            | TypeKind::Typeof(_)
            | TypeKind::KeyOf(_)
            | TypeKind::IndexedAccess { .. }
            | TypeKind::Mapped { .. }
            | TypeKind::Conditional { .. }
            | TypeKind::Infer(_)
            | TypeKind::EnumVariant { .. }
            | TypeKind::TypePredicate { .. } => *self,
        }
    }

    pub fn minus(&self, other: &Type, table: &mut CheckerTyTable) -> Type {
        if self == other {
            return Type::Never;
        }
        match table.get(self.0) {
            TypeKind::Union(list) => {
                let members: Vec<CheckerTyId> = table.get_list(list).to_vec();
                let dynamic_array = {
                    let arr = Type::array(Type::Dynamic, table);
                    arr.0
                };
                let mut kept: Vec<CheckerTyId> = Vec::with_capacity(members.len());
                for id in members {
                    if id == other.0 {
                        continue;
                    }
                    if let (TypeKind::Array(_), TypeKind::Array(_)) =
                        (table.get(id), table.get(other.0))
                        && (other.0 == dynamic_array || id == other.0)
                    {
                        continue;
                    }
                    kept.push(id);
                }
                if kept.is_empty() {
                    Type::Never
                } else if kept.len() == 1 {
                    Type::resolved(kept[0])
                } else {
                    let new_list = table.intern_list(&kept);
                    Type::resolved(table.intern(TypeKind::Union(new_list)))
                }
            }
            TypeKind::Primitive(_)
            | TypeKind::Literal(_)
            | TypeKind::This
            | TypeKind::Array(_)
            | TypeKind::Intersection(_)
            | TypeKind::Tuple(_)
            | TypeKind::Named(..)
            | TypeKind::Generic(..)
            | TypeKind::TemplateLiteral(_)
            | TypeKind::Fn(_)
            | TypeKind::Record(_)
            | TypeKind::Interface(_)
            | TypeKind::Typeof(_)
            | TypeKind::KeyOf(_)
            | TypeKind::IndexedAccess { .. }
            | TypeKind::Mapped { .. }
            | TypeKind::Conditional { .. }
            | TypeKind::Infer(_)
            | TypeKind::EnumVariant { .. }
            | TypeKind::TypePredicate { .. } => *self,
        }
    }

    pub fn map_generics(
        &self,
        mapping: &FxHashMap<varn_core::Atom, Type>,
        table: &mut CheckerTyTable,
    ) -> Type {
        match table.get(self.0) {
            TypeKind::Named(n, _) => {
                if let Some(t) = mapping.get(&n) {
                    return *t;
                }
                *self
            }
            TypeKind::Generic(n, args, origin) => {
                let arg_ids = table.get_list(args).to_vec();
                let new_args: Vec<CheckerTyId> = arg_ids
                    .into_iter()
                    .map(|a| Type::resolved(a).map_generics(mapping, table).0)
                    .collect();
                let new_list = table.intern_list(&new_args);
                Type(table.intern(TypeKind::Generic(n, new_list, origin)))
            }
            TypeKind::Array(inner) => {
                let mapped = Type::resolved(inner).map_generics(mapping, table);
                Type::array(mapped, table)
            }
            TypeKind::Union(list) => {
                let ids = table.get_list(list).to_vec();
                let new_members: Vec<Type> = ids
                    .into_iter()
                    .map(|id| Type::resolved(id).map_generics(mapping, table))
                    .collect();
                Type::union(new_members, table)
            }
            TypeKind::Fn(fid) => {
                let ft = table.get_function(fid).clone();
                let mut new_params: Vec<FunctionParam> = Vec::with_capacity(ft.params.len());
                for p in &ft.params {
                    if p.is_rest
                        && let Some(slots) = rest_pack_slots(p.ty, mapping, table)
                    {
                        for (at, slot) in slots.into_iter().enumerate() {
                            new_params.push(FunctionParam {
                                name: Some(std::sync::Arc::from(format!("arg{at}"))),
                                ty: slot,
                                optional: false,
                                is_rest: false,
                            });
                        }
                        continue;
                    }
                    new_params.push(FunctionParam {
                        name: p.name.clone(),
                        ty: Type::resolved(p.ty).map_generics(mapping, table).0,
                        optional: p.optional,
                        is_rest: p.is_rest,
                    });
                }
                let new_ret = Type::resolved(ft.return_type)
                    .map_generics(mapping, table)
                    .0;
                Type::fn_(
                    FunctionType {
                        params: new_params,
                        return_type: new_ret,
                        is_arrow: ft.is_arrow,
                        type_params: ft.type_params.clone(),
                        type_param_defaults: ft.type_param_defaults.clone(),
                    },
                    table,
                )
            }
            TypeKind::Record(mid) => {
                let members = table.get_object_members(mid).to_vec();
                let new_members: Vec<ObjectTypeMember> = members
                    .into_iter()
                    .map(|m| m.map_generics(mapping, table))
                    .collect();
                Type::record(new_members, table)
            }
            TypeKind::Interface(mid) => {
                let members = table.get_object_members(mid).to_vec();
                let new_members: Vec<ObjectTypeMember> = members
                    .into_iter()
                    .map(|m| m.map_generics(mapping, table))
                    .collect();
                Type::interface(new_members, table)
            }
            TypeKind::Tuple(els) => {
                let ids = table.get_list(els).to_vec();
                let new_els: Vec<CheckerTyId> = ids
                    .into_iter()
                    .map(|id| Type::resolved(id).map_generics(mapping, table).0)
                    .collect();
                let new_list = table.intern_list(&new_els);
                Type(table.intern(TypeKind::Tuple(new_list)))
            }
            TypeKind::Conditional {
                check,
                extends,
                true_type,
                false_type,
            } => {
                let new_check = Type::resolved(check).map_generics(mapping, table);
                let new_extends = Type::resolved(extends).map_generics(mapping, table);
                let new_true = Type::resolved(true_type).map_generics(mapping, table);
                let new_false = Type::resolved(false_type).map_generics(mapping, table);
                if new_check.0 != check {
                    crate::resolve::evaluate_conditional(
                        new_check,
                        new_extends,
                        new_true,
                        new_false,
                        table,
                    )
                } else {
                    Type::resolved(table.intern(TypeKind::Conditional {
                        check: new_check.0,
                        extends: new_extends.0,
                        true_type: new_true.0,
                        false_type: new_false.0,
                    }))
                }
            }
            TypeKind::Primitive(_)
            | TypeKind::Literal(_)
            | TypeKind::This
            | TypeKind::Intersection(_)
            | TypeKind::TemplateLiteral(_)
            | TypeKind::Typeof(_)
            | TypeKind::KeyOf(_)
            | TypeKind::IndexedAccess { .. }
            | TypeKind::Mapped { .. }
            | TypeKind::Infer(_)
            | TypeKind::EnumVariant { .. }
            | TypeKind::TypePredicate { .. } => *self,
        }
    }

    pub fn qualify_nominals(
        self,
        origin: varn_core::Atom,
        is_nominal: &dyn Fn(varn_core::Atom) -> bool,
        table: &mut CheckerTyTable,
    ) -> Self {
        match table.get(self.0) {
            TypeKind::Named(n, None) if is_nominal(n) => {
                Type(table.intern(TypeKind::Named(n, Some(origin))))
            }
            TypeKind::Generic(n, args, current) => {
                let ids = table.get_list(args).to_vec();
                let new_args: Vec<CheckerTyId> = ids
                    .into_iter()
                    .map(|a| {
                        Type::resolved(a)
                            .qualify_nominals(origin, is_nominal, table)
                            .0
                    })
                    .collect();
                let list = table.intern_list(&new_args);
                let origin = current.or_else(|| is_nominal(n).then_some(origin));
                Type(table.intern(TypeKind::Generic(n, list, origin)))
            }
            TypeKind::Array(inner) => {
                let inner = Type::resolved(inner).qualify_nominals(origin, is_nominal, table);
                Type::array(inner, table)
            }
            TypeKind::Union(list) => {
                let ids = table.get_list(list).to_vec();
                let members: Vec<Type> = ids
                    .into_iter()
                    .map(|id| Type::resolved(id).qualify_nominals(origin, is_nominal, table))
                    .collect();
                Type::union(members, table)
            }
            TypeKind::Tuple(els) => {
                let ids = table.get_list(els).to_vec();
                let new_els: Vec<CheckerTyId> = ids
                    .into_iter()
                    .map(|id| {
                        Type::resolved(id)
                            .qualify_nominals(origin, is_nominal, table)
                            .0
                    })
                    .collect();
                let list = table.intern_list(&new_els);
                Type(table.intern(TypeKind::Tuple(list)))
            }
            TypeKind::Fn(fid) => {
                let ft = table.get_function(fid).clone();
                let params = ft
                    .params
                    .iter()
                    .map(|p| FunctionParam {
                        name: p.name.clone(),
                        ty: Type::resolved(p.ty)
                            .qualify_nominals(origin, is_nominal, table)
                            .0,
                        optional: p.optional,
                        is_rest: p.is_rest,
                    })
                    .collect();
                let return_type = Type::resolved(ft.return_type)
                    .qualify_nominals(origin, is_nominal, table)
                    .0;
                Type::fn_(
                    FunctionType {
                        params,
                        return_type,
                        is_arrow: ft.is_arrow,
                        type_params: ft.type_params.clone(),
                        type_param_defaults: ft.type_param_defaults.clone(),
                    },
                    table,
                )
            }
            TypeKind::Record(mid) => {
                let members = table.get_object_members(mid).to_vec();
                let members: Vec<ObjectTypeMember> = members
                    .into_iter()
                    .map(|m| {
                        m.map_types(table, &mut |ty, table| {
                            ty.qualify_nominals(origin, is_nominal, table)
                        })
                    })
                    .collect();
                Type::record(members, table)
            }
            TypeKind::Interface(mid) => {
                let members = table.get_object_members(mid).to_vec();
                let members: Vec<ObjectTypeMember> = members
                    .into_iter()
                    .map(|m| {
                        m.map_types(table, &mut |ty, table| {
                            ty.qualify_nominals(origin, is_nominal, table)
                        })
                    })
                    .collect();
                Type::interface(members, table)
            }
            TypeKind::Named(..)
            | TypeKind::Primitive(_)
            | TypeKind::Literal(_)
            | TypeKind::This
            | TypeKind::Intersection(_)
            | TypeKind::TemplateLiteral(_)
            | TypeKind::Typeof(_)
            | TypeKind::KeyOf(_)
            | TypeKind::IndexedAccess { .. }
            | TypeKind::Mapped { .. }
            | TypeKind::Conditional { .. }
            | TypeKind::Infer(_)
            | TypeKind::EnumVariant { .. }
            | TypeKind::TypePredicate { .. } => self,
        }
    }

    pub fn with_origin(self, origin: varn_core::Atom, table: &mut CheckerTyTable) -> Self {
        match table.get(self.0) {
            TypeKind::Named(n, None) => Type(table.intern(TypeKind::Named(n, Some(origin)))),
            TypeKind::Generic(n, args, None) => {
                Type(table.intern(TypeKind::Generic(n, args, Some(origin))))
            }
            TypeKind::Named(_, Some(_)) | TypeKind::Generic(_, _, Some(_)) => self,
            TypeKind::Primitive(_)
            | TypeKind::Literal(_)
            | TypeKind::This
            | TypeKind::Array(_)
            | TypeKind::Union(_)
            | TypeKind::Intersection(_)
            | TypeKind::Tuple(_)
            | TypeKind::TemplateLiteral(_)
            | TypeKind::Fn(_)
            | TypeKind::Record(_)
            | TypeKind::Interface(_)
            | TypeKind::Typeof(_)
            | TypeKind::KeyOf(_)
            | TypeKind::IndexedAccess { .. }
            | TypeKind::Mapped { .. }
            | TypeKind::Conditional { .. }
            | TypeKind::Infer(_)
            | TypeKind::EnumVariant { .. }
            | TypeKind::TypePredicate { .. } => self,
        }
    }
}

fn rest_pack_slots(
    rest_ty: CheckerTyId,
    mapping: &FxHashMap<varn_core::Atom, Type>,
    table: &CheckerTyTable,
) -> Option<Vec<CheckerTyId>> {
    let element = if let TypeKind::Array(inner) = table.get(rest_ty) {
        inner
    } else {
        rest_ty
    };
    let TypeKind::Named(name, _) = table.get(element) else {
        return None;
    };
    let packed = mapping.get(&name)?;
    let TypeKind::Tuple(slots) = table.get(packed.0) else {
        return None;
    };
    Some(table.get_list(slots).to_vec())
}
